// PCE Mini Recovery — frontend glue.
// Tauri v2 with `withGlobalTauri: true` exposes invoke/listen on window.__TAURI__.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ---- the canonical step list -------------------------------------------------
//
// The order here drives the on-screen stepper. Each id matches the `phase`
// field emitted by the Rust backend (recovery::PhaseId — serialized as the
// variant name by serde, so casing matches).

const STEPS = [
  { id: "Connect",         label: "Wait for console",         showBytes: false },
  { id: "Trigger",         label: "Enter recovery mode",                 showBytes: false },
  { id: "Version",         label: "Identify console",          showBytes: false },
  { id: "Fes1Write",       label: "Load memory initializer",          showBytes: true  },
  { id: "Fes1Exec",        label: "Initialize console memory",                showBytes: false },
  { id: "BootImgWrite",    label: "Load recovery environment",          showBytes: true  },
  { id: "UbootWrite",      label: "Load recovery bootloader",            showBytes: true  },
  { id: "UbootExec",       label: "Start recovery",      showBytes: false },
];

// ---- step rendering ---------------------------------------------------------

const stepsEl = document.getElementById("steps");
const stepNodes = new Map();

function buildSteps() {
  stepsEl.innerHTML = "";
  stepNodes.clear();
  for (const s of STEPS) {
    const li = document.createElement("li");
    li.className = "step pending";
    li.dataset.id = s.id;
    li.innerHTML = `
      <span class="step-icon">○</span>
      <span class="step-label">${s.label}</span>
      <span class="step-detail">—</span>
      ${s.showBytes ? `<div class="step-bar"><div></div></div>` : ""}
    `;
    stepsEl.appendChild(li);
    stepNodes.set(s.id, {
      el: li,
      icon: li.querySelector(".step-icon"),
      detail: li.querySelector(".step-detail"),
      bar: li.querySelector(".step-bar > div"),
      total: 0,
      progress: 0,
      startedAt: 0,
    });
  }
}

function setStepState(id, state) {
  const n = stepNodes.get(id);
  if (!n) return;
  n.el.classList.remove("pending", "running", "done", "error");
  n.el.classList.add(state);
  n.icon.textContent =
    state === "running" ? "↻" :
    state === "done"    ? "✓" :
    state === "error"   ? "!" : "○";
}

function fmtBytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  return `${(n / 1024 / 1024).toFixed(2)} MiB`;
}

let busy = false;
let batch = false;
let refreshingNetwork = false;
function setBusy(value) {
  busy = value;
  for (const el of document.querySelectorAll('#start, #network-connect, #network-refresh, #dump-all, #payloads-browse, .dump, .restore')) el.disabled = value;
}
function setDeviceState(text, kind) {
  const el = document.getElementById('device-state');
  el.textContent = text; el.className = `pill pill-${kind}`;
}
function log(message) {
  const el=document.getElementById('activity');
  el.textContent=(el.textContent + message + '\n').slice(-24000);
  el.scrollTop=el.scrollHeight;
}
async function refreshNetwork() {
  if (refreshingNetwork) return;
  refreshingNetwork=true;
  try {
    const interfaces = await invoke('network_interfaces');
    const select=document.getElementById('network-interface');
    const previous=select.value;
    select.replaceChildren();
    const placeholder=document.createElement('option');placeholder.value='';
    placeholder.textContent=interfaces.length ? 'Select the console USB interface…' : 'Waiting for USB network…';
    select.appendChild(placeholder);
    for(const iface of interfaces) {
      const option=document.createElement('option');option.value=iface.name;
      option.textContent=`${iface.name} · ${iface.product || 'PCE Recovery'}`;select.appendChild(option);
    }
    if(interfaces.some(i=>i.name===previous)) select.value=previous;
    else if(interfaces.length===1) select.value=interfaces[0].name;
  } catch(e) { log(`USB network: ${e}`); }
  finally { refreshingNetwork=false; }
}
async function connectNetwork() {
  if(busy) return;
  const interfaceName=document.getElementById('network-interface').value;
  if(!interfaceName) { log('Wait for the recovery USB network interface.');return; }
  setBusy(true);
  try { await invoke('network_connect',{interface:interfaceName});log('USB network configured. Waiting for the console at 169.254.13.37…'); }
  catch(e) { log(`Network setup: ${e}`); }
  finally { setBusy(false); }
}
async function wireBackend() {
  await listen('phase-start',e=>{
    const {phase,total,label}=e.payload;const n=stepNodes.get(phase);log(label);
    if(phase==='Connect') {
      document.getElementById('recovery-prompt').textContent='Ready — switch the console ON now. Waiting for its startup USB probe…';
      setDeviceState('Switch console ON','amber');
    } else {
      document.getElementById('recovery-prompt').textContent='Console detected. Recovery startup is running automatically; keep the USB cable connected.';
      setDeviceState('Starting recovery…','amber');
    }
    if(!n)return;n.total=total;n.progress=0;n.startedAt=performance.now();setStepState(phase,'running');n.detail.textContent='Working…';
  });
  await listen('phase-advance',e=>{
    const {phase,delta}=e.payload;const n=stepNodes.get(phase);if(!n)return;n.progress+=delta;
    if(n.bar)n.bar.style.width=`${Math.min(100,100*n.progress/n.total)}%`;
    n.detail.textContent=`${fmtBytes(n.progress)} / ${fmtBytes(n.total)}`;
  });
  await listen('phase-finish',e=>{setStepState(e.payload.phase,'done');const n=stepNodes.get(e.payload.phase);if(n)n.detail.textContent='Done';});
  await listen('fel-version',e=>{
    document.getElementById('version-card').hidden=false;
    document.getElementById('version-info').textContent=`${e.payload.soc_name} · FEL protocol`;
  });
  await listen('net-probe',e=>{
    const up=e.payload.state==='up';document.getElementById('ping-light').className=`ping-light ${up?'up':'down'}`;
    document.getElementById('ping-label').textContent=up?'Console connected':'Console offline';
  });
  await listen('recovery-done',e=>{
    const {ok,msg}=e.payload;
    const prompt=document.getElementById('recovery-prompt');
    prompt.textContent=msg;prompt.className=ok?'recovery-prompt':'recovery-prompt error';
    if(!ok) for(const [id,node] of stepNodes) {
      if(node.el.classList.contains('running')) {setStepState(id,'error');node.detail.textContent='Failed';}
    }
    setBusy(false);setDeviceState(ok?'Recovery boot sent':'Recovery failed',ok?'green':'red');log(msg);refreshNetwork();
  });
  await listen('log',e=>log(e.payload.msg));
  await listen('partition-progress',e=>{
    const {id,kind,current,total}=e.payload;const cell=document.querySelector(`#partitions-table tr[data-id="${id}"] .row-progress`);
    if(cell) {cell.classList.remove('ok','err');cell.textContent=`${kind}: ${Math.min(100,100*current/total).toFixed(0)}% · ${fmtBytes(current)} / ${fmtBytes(total)}`;}
  });
  await listen('partition-done',e=>{
    const {id,ok,kind,msg}=e.payload;const cell=document.querySelector(`#partitions-table tr[data-id="${id}"] .row-progress`);
    if(cell){cell.classList.add(ok?'ok':'err');cell.textContent=ok?`${kind} verified ✓`:`${kind}: ${msg}`;}
    log(`P${id}: ${ok?kind+' verified':msg}`);if(!batch)setBusy(false);
  });
  await listen('partition-batch',e=>{
    const p=e.payload;const el=document.getElementById('batch-status');
    if(p.state==='start'){batch=true;setBusy(true);el.textContent=`Backing up ${p.total} partitions…`;}
    if(p.state==='next')el.textContent=`${p.index}/${p.total} · P${p.id}`;
    if(p.state==='done'||p.state==='error'){batch=false;setBusy(false);el.textContent=p.state==='error'?p.msg:`${p.total-p.failed} verified, ${p.failed} failed`;}
  });
}
async function buildPartitions() {
  const parts = await invoke("partitions_list");
  const tbody = document.querySelector("#partitions-table tbody");
  tbody.innerHTML = "";
  for (const p of parts) {
    const tr = document.createElement("tr");
    tr.dataset.id = p.id;
    if (p.id === 0) tr.classList.add("full");
    tr.innerHTML = `
      <td>${p.id === 0 ? "*" : p.id}</td>
      <td class="device">${p.device_path}</td>
      <td class="size right">${fmtBytes(p.size_bytes)}</td>
      <td>${p.role}</td>
      <td class="actions">
        <button class="dump">Dump</button>
        <button class="restore">Restore</button>
        <span class="row-progress"></span>
      </td>
    `;
    tr.querySelector(".dump").addEventListener("click", () => doDump(p));
    tr.querySelector(".restore").addEventListener("click", () => doRestore(p));
    tbody.appendChild(tr);
  }
}

async function doDump(p) {
  if(busy)return;setBusy(true);
  try {
    const defaultName=p.id===0?'full_nand.bin':`mmcblk0p${p.id}.bin`;
    const path=await invoke('pick_save_file',{defaultName});
    if(!path){setBusy(false);return;}
    await invoke('partition_dump',{id:p.id,outPath:path});
  } catch(e) {log(`Backup: ${e}`);setBusy(false);}
}
async function doDumpAll() {
  if(busy)return;setBusy(true);
  try {
    const dir=await invoke('pick_directory');if(!dir){setBusy(false);return;}
    batch=true;await invoke('partition_dump_all',{dir});
  } catch(e) {log(`Backup: ${e}`);batch=false;setBusy(false);}
}
async function doRestore(p) {
  if (busy) return;
  setBusy(true);
  const row = document.querySelector(`#partitions-table tr[data-id="${p.id}"]`);
  const cell = row.querySelector(".row-progress");
  const buttons = row.querySelectorAll("button");
  const cleanup = () => {
    cell.replaceChildren();
    setBusy(false);
  };
  const showError = (e) => {
    cell.classList.add("err");
    cell.textContent = `restore ✗ ${e}`;
    setBusy(false);
  };
  buttons.forEach((b) => (b.disabled = true));
  cell.classList.remove("ok", "err");

  let path, entries;
  try {
    path = await invoke("pick_open_file");
    if (!path) { cleanup(); return; }
    cell.textContent = "Inspecting image…";
    entries = await invoke("partition_inspect_image", { id: p.id, inPath: path });
    if (!entries.length) throw new Error("No matching image in this archive");
  } catch (e) {
    showError(e);
    return;
  }

  cell.replaceChildren();
  const source = document.createElement("span");
  source.className = "restore-source";
  source.textContent = path.split(/[\\/]/).pop();
  source.title = path;
  cell.appendChild(source);

  let selection = entries.length === 1 ? entries[0] : null;
  const confirm = document.createElement("button");
  confirm.className = "confirm-yes";
  confirm.textContent = "Confirm";
  confirm.disabled = !selection;

  if (entries.length > 1) {
    const select = document.createElement("select");
    select.className = "restore-entry";
    select.setAttribute("aria-label", "Image inside ZIP archive");
    const placeholder = document.createElement("option");
    placeholder.value = "";
    placeholder.textContent = "Choose the image to restore…";
    select.appendChild(placeholder);
    entries.forEach((entry, index) => {
      const option = document.createElement("option");
      option.value = String(index);
      option.textContent = `${entry.name} (${fmtBytes(entry.size)})`;
      select.appendChild(option);
    });
    select.addEventListener("change", () => {
      selection = select.value === "" ? null : entries[Number(select.value)];
      confirm.disabled = !selection;
    });
    cell.appendChild(select);
  } else if (selection.zip_index !== null) {
    const member = document.createElement("span");
    member.className = "restore-source";
    member.textContent = `Image: ${selection.name}`;
    cell.appendChild(member);
  }

  const warning = document.createElement("span");
  warning.className = "confirm-warn";
  warning.textContent = `⚠ overwrite ${fmtBytes(p.size_bytes)} on ${p.device_path}?`;
  cell.appendChild(warning);
  cell.appendChild(confirm);
  const cancel = document.createElement("button");
  cancel.className = "confirm-no";
  cancel.textContent = "Cancel";
  cell.appendChild(cancel);
  cancel.addEventListener("click", cleanup);
  // No timeout while the user is reading member names or choosing an image.
  confirm.addEventListener("click", async () => {
    if (!selection || confirm.disabled) return;
    confirm.disabled = true;
    cell.textContent = "Checking image before restore…";
    try {
      await invoke("partition_restore", { id: p.id, inPath: path, selection });
    } catch (e) {
      showError(e);
    }
  });
}

// ---- recovery controls ----
async function startRecovery() {
  if(busy)return;
  const payloadsDir=document.getElementById('payloads-dir').value.trim();
  if(!payloadsDir){log('Choose the recovery files folder first.');return;}
  const waitSecs=parseInt(document.getElementById('wait-secs').value,10)||120;
  buildSteps();setBusy(true);setDeviceState('Preparing detection…','amber');
  document.getElementById('version-card').hidden=true;
  document.getElementById('recovery-prompt').className='recovery-prompt';
  document.getElementById('recovery-prompt').textContent='Keep the console OFF while detection is being prepared…';
  try {await invoke('start_recovery',{payloadsDir,waitSecs});}
  catch(e){log(`Recovery: ${e}`);document.getElementById('recovery-prompt').textContent=`Recovery failed: ${e}`;setDeviceState('Recovery failed','red');setBusy(false);}
}
(async()=>{
  buildSteps();await wireBackend();await buildPartitions();
  document.getElementById('payloads-dir').value=await invoke('default_payloads_dir');
  document.getElementById('start').addEventListener('click',startRecovery);
  document.getElementById('network-connect').addEventListener('click',connectNetwork);
  document.getElementById('network-refresh').addEventListener('click',refreshNetwork);
  document.getElementById('dump-all').addEventListener('click',doDumpAll);
  document.getElementById('payloads-browse').addEventListener('click',async()=>{const dir=await invoke('pick_directory');if(dir)document.getElementById('payloads-dir').value=dir;});
  await refreshNetwork();setInterval(()=>{if(!busy)refreshNetwork();},2500);
})().catch(e=>log(`Startup: ${e}`));
