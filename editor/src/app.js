const { invoke } = window.__TAURI__.core;
const { open: dialogOpen } = window.__TAURI__.dialog;
const { getCurrentWebview } = window.__TAURI__.webview;

let dualLibrary = null;   // { jp: {games, path}, us: {games, path}, path }
let currentLineup = "jp"; // "jp" or "us"
let selectedIndex = -1;
let saveTimeout = null; // debounce timer for auto-save
let coverCache = {};       // "lineup/folder" -> base64 data URL
let currentFolder = null;  // null or { lineup, name, parentGames }
let editorSettings = { confirm_delete: true, force_us_titlebar: true, games_path: null };
let gamesSettings = { genres: [], alpha_exclusions: [] };
let genreNames = ['\u2014']; // index 0 = unset, rest populated from settings

// Current lineup helper
function getLibrary() {
    return dualLibrary ? dualLibrary[currentLineup] : null;
}

// ---- Modal helpers (Tauri WKWebView doesn't support prompt/confirm/alert) ----
function showModal(title, { hint, defaultValue, showInput } = {}) {
    return new Promise((resolve) => {
        const overlay = document.getElementById('modal-overlay');
        const input = document.getElementById('modal-input');
        const titleEl = document.getElementById('modal-title');
        const hintEl = document.getElementById('modal-hint');
        const okBtn = document.getElementById('modal-ok');
        const cancelBtn = document.getElementById('modal-cancel');

        titleEl.textContent = title;
        hintEl.textContent = hint || '';
        hintEl.classList.remove('error');
        input.style.display = showInput ? '' : 'none';
        input.value = defaultValue || '';
        overlay.classList.add('visible');
        if (showInput) input.focus();

        function cleanup() {
            overlay.classList.remove('visible');
            okBtn.removeEventListener('click', onOk);
            cancelBtn.removeEventListener('click', onCancel);
            input.removeEventListener('keydown', onKey);
        }
        function onOk() { cleanup(); resolve(showInput ? input.value : true); }
        function onCancel() { cleanup(); resolve(showInput ? null : false); }
        function onKey(e) {
            if (e.key === 'Enter') onOk();
            if (e.key === 'Escape') onCancel();
        }
        okBtn.addEventListener('click', onOk);
        cancelBtn.addEventListener('click', onCancel);
        input.addEventListener('keydown', onKey);
    });
}

function modalPrompt(title, hint) {
    return showModal(title, { hint, showInput: true });
}

function modalConfirm(title) {
    return showModal(title);
}

function modalAlert(msg) {
    return showModal(msg);
}

// Modal variant that shows a list of buttons instead of a text input.
// options: [{ label: string, value: any }, ...]. Returns selected value or null on cancel.
function modalSelect(title, options) {
    return new Promise((resolve) => {
        const overlay = document.getElementById('modal-overlay');
        const titleEl = document.getElementById('modal-title');
        const selectList = document.getElementById('modal-select-list');
        const cancelBtn = document.getElementById('modal-cancel');
        const okBtn = document.getElementById('modal-ok');
        const input = document.getElementById('modal-input');
        const hintEl = document.getElementById('modal-hint');

        titleEl.textContent = title;
        hintEl.textContent = '';
        input.style.display = 'none';
        okBtn.style.display = 'none';
        selectList.innerHTML = '';

        options.forEach(opt => {
            const btn = document.createElement('button');
            btn.className = 'modal-select-btn';
            btn.textContent = opt.label;
            btn.addEventListener('click', () => { cleanup(); resolve(opt.value); });
            selectList.appendChild(btn);
        });

        overlay.classList.add('visible');

        function cleanup() {
            overlay.classList.remove('visible');
            cancelBtn.removeEventListener('click', onCancel);
            selectList.innerHTML = '';
            okBtn.style.display = '';
        }
        function onCancel() { cleanup(); resolve(null); }
        cancelBtn.addEventListener('click', onCancel);
    });
}

// ---- Settings ----
async function loadSettings() {
    try {
        // TEMP: surface flow in titlebar + console so we can debug what
        // path the auto-init takes on launch. Remove once verified.
        const dbg = (m) => { console.log('[init]', m); document.title = 'PCE Editor — ' + m; };
        dbg('loading settings...');
        editorSettings = await invoke('load_editor_settings');
        document.getElementById('s-confirm-delete').checked = editorSettings.confirm_delete;
        document.getElementById('s-force-us-titlebar').checked = editorSettings.force_us_titlebar;
        // Auto-discover the library by looking at the editor's own location.
        // In the deployed workflow the editor binary sits at the USB root,
        // alongside library/, published/, templates/, BACKUP/. In dev
        // (`npm run dev`) the exe is in target/debug — fall back to the
        // saved games_path so the dev loop still works.
        let root = null;
        let exeRoot = null;
        try {
            exeRoot = await invoke('get_editor_root');
            dbg('exe at ' + exeRoot);
            const exeStatus = await invoke('check_library_status', { usbRoot: exeRoot });
            dbg('exe status: lib=' + exeStatus.has_library + ' backup=' + exeStatus.has_backup);
            if (exeStatus.has_library || exeStatus.has_backup) {
                root = exeRoot;
            }
        } catch (e) {
            console.warn('get_editor_root failed:', e);
            modalAlert('get_editor_root failed: ' + e);
        }
        if (!root && editorSettings.games_path) {
            root = editorSettings.games_path;
            dbg('falling back to saved path ' + root);
        }
        if (!root) {
            dbg('no root, idle (user can Open Library manually)');
            return;
        }
        updateGamesPathDisplay(root);
        try {
            const status = await invoke('check_library_status', { usbRoot: root });
            dbg('root status: lib=' + status.has_library + ' backup=' + status.has_backup
                + (status.missing_templates && status.missing_templates.length
                    ? ' MISSING=' + status.missing_templates.join(',') : ''));
            if (status.has_library) {
                dbg('loading library');
                await loadLibrary(root);
            } else if (status.has_backup
                && (!status.missing_templates || !status.missing_templates.length)) {
                dbg('prompting to init');
                const ok = await modalConfirm(
                    'No library found on this USB key. Initialize an empty library now?'
                );
                if (ok) {
                    try {
                        dbg('init_library...');
                        const initRes = await invoke('init_library', { usbRoot: root });
                        dbg('publishing...');
                        await invoke('publish_library', {
                            gamesPath: initRes.library_root,
                            stockDataRoot: '',
                            outputRoot: '',
                        });
                        dbg('loading library');
                        await loadLibrary(root);
                        onSettingChange('games_path', root);
                        dbg('ready');
                    } catch (e) {
                        modalAlert('Library initialization failed: ' + e);
                    }
                }
            } else {
                modalAlert('No library and no usable BACKUP/game/ found at ' + root +
                    (status.missing_templates && status.missing_templates.length
                        ? '\nMissing: ' + status.missing_templates.join('\n        ') : ''));
            }
        } catch (e) {
            console.warn('Could not auto-load library:', e);
            modalAlert('Auto-load failed: ' + e);
        }
    } catch (e) {
        console.error('Failed to load settings:', e);
    }
}

async function loadGamesSettings(path) {
    try {
        gamesSettings = await invoke('load_games_settings', { gamesPath: path });
    } catch (e) {
        console.error('Failed to load games settings:', e);
        gamesSettings = { genres: [], alpha_exclusions: [] };
    }
    syncGenreNames();
    renderGenreSettings();
    renderAlphaExclusionSettings();
    rebuildGenreDropdown();
}

function syncGenreNames() {
    genreNames = ['\u2014', ...(gamesSettings.genres || [])];
}

function rebuildGenreDropdown() {
    const sel = document.getElementById('f-genre');
    sel.innerHTML = '';
    genreNames.forEach((name, i) => {
        const opt = document.createElement('option');
        opt.value = i;
        opt.textContent = name;
        sel.appendChild(opt);
    });
}

function renderGenreSettings() {
    const list = document.getElementById('s-genre-list');
    list.innerHTML = '';
    (gamesSettings.genres || []).forEach((name, i) => {
        const row = document.createElement('div');
        row.className = 'setting-list-item';
        const label = document.createElement('span');
        label.className = 'setting-list-label';
        label.textContent = name;
        label.addEventListener('dblclick', () => {
            const input = document.createElement('input');
            input.type = 'text';
            input.value = name;
            input.className = 'setting-list-edit';
            row.replaceChild(input, label);
            input.focus();
            input.select();
            const finish = () => {
                const newName = input.value.trim();
                if (newName && newName !== name) {
                    gamesSettings.genres[i] = newName;
                    syncGenreNames();
                    rebuildGenreDropdown();
                    saveGamesSettingsToBackend();
                }
                renderGenreSettings();
            };
            input.addEventListener('blur', finish);
            input.addEventListener('keydown', (e) => {
                if (e.key === 'Enter') input.blur();
                if (e.key === 'Escape') { input.value = name; input.blur(); }
            });
        });
        const del = document.createElement('button');
        del.className = 'btn-small btn-list-delete';
        del.textContent = '\u00d7';
        del.title = 'Delete genre';
        del.addEventListener('click', () => deleteGenre(i));
        row.appendChild(label);
        row.appendChild(del);
        list.appendChild(row);
    });
}

function renderAlphaExclusionSettings() {
    const list = document.getElementById('s-alpha-list');
    list.innerHTML = '';
    (gamesSettings.alpha_exclusions || []).forEach((prefix, i) => {
        const row = document.createElement('div');
        row.className = 'setting-list-item';
        const label = document.createElement('span');
        label.className = 'setting-list-label';
        label.textContent = prefix;
        const del = document.createElement('button');
        del.className = 'btn-small btn-list-delete';
        del.textContent = '\u00d7';
        del.addEventListener('click', () => {
            gamesSettings.alpha_exclusions.splice(i, 1);
            renderAlphaExclusionSettings();
            saveGamesSettingsToBackend();
            if (dualLibrary) {
                recomputeSortIndices(dualLibrary.jp.games);
                recomputeSortIndices(dualLibrary.us.games);
                autoSave();
            }
        });
        row.appendChild(label);
        row.appendChild(del);
        list.appendChild(row);
    });
}

async function saveEditorSettingsToBackend() {
    try {
        await invoke('save_editor_settings', { settings: editorSettings });
    } catch (e) {
        console.error('Failed to save editor settings:', e);
    }
}

async function saveGamesSettingsToBackend() {
    if (!editorSettings.games_path) return;
    try {
        await invoke('save_games_settings', { gamesPath: editorSettings.games_path, settings: gamesSettings });
    } catch (e) {
        console.error('Failed to save games settings:', e);
    }
}

function deleteGenre(index) {
    const genreId = index + 1; // genre IDs are 1-based
    // Update all games in both lineups
    if (dualLibrary) {
        [dualLibrary.jp, dualLibrary.us].forEach(lib => {
            if (!lib) return;
            lib.games.forEach(entry => {
                if (entry.is_folder) return;
                const g = entry.game.genre;
                if (g === genreId) {
                    entry.game.genre = null;
                } else if (g && g > genreId) {
                    entry.game.genre = g - 1;
                }
            });
            recomputeSortIndices(lib.games);
        });
        autoSave();
    }
    gamesSettings.genres.splice(index, 1);
    syncGenreNames();
    rebuildGenreDropdown();
    renderGenreSettings();
    saveGamesSettingsToBackend();
    // Refresh selected game's genre dropdown
    const library = getLibrary();
    if (library && selectedIndex >= 0 && !library.games[selectedIndex].is_folder) {
        setField('f-genre', library.games[selectedIndex].game.genre || 0);
    }
}

function updateGamesPathDisplay(path) {
    const el = document.getElementById('s-games-path');
    el.textContent = path || 'Not set';
    el.title = path || '';
}

function openSettings() {
    if (document.getElementById('settings-panel').style.display !== 'none') {
        closeSettings();
        return;
    }
    document.getElementById('editor').classList.remove('active');
    document.getElementById('folder-editor').style.display = 'none';
    document.getElementById('sorting-panel').style.display = 'none';
    document.getElementById('placeholder').style.display = 'none';
    document.getElementById('settings-panel').style.display = '';
    selectedIndex = -1;
    document.querySelectorAll('.game-item').forEach(el => el.classList.remove('selected'));
}

function closeSettings() {
    document.getElementById('settings-panel').style.display = 'none';
    document.getElementById('placeholder').style.display = '';
}

// ---- Sort Editor ----
function openSortEditor() {
    if (!dualLibrary) return;
    document.getElementById('editor').classList.remove('active');
    document.getElementById('folder-editor').style.display = 'none';
    document.getElementById('settings-panel').style.display = 'none';
    document.getElementById('placeholder').style.display = 'none';
    document.getElementById('sorting-panel').style.display = '';
    selectedIndex = -1;
    document.querySelectorAll('.game-item').forEach(el => el.classList.remove('selected'));
    // Default to date tab
    const activeTab = document.querySelector('.sort-tab.active');
    renderSortList(activeTab ? activeTab.dataset.sort : 'date');
}

function closeSortEditor() {
    document.getElementById('sorting-panel').style.display = 'none';
    document.getElementById('placeholder').style.display = '';
}

// Shared pointer-based drag reorder for sort lists (swap-in-place)
function setupDragReorder(list, entries, library, sortField) {
    let dragState = null;

    list.addEventListener('pointerdown', (e) => {
        const row = e.target.closest('.sort-item-draggable');
        if (!row) return;
        e.preventDefault();
        row.setPointerCapture(e.pointerId);
        row.classList.add('dragging');
        dragState = { row };
    });

    list.addEventListener('pointermove', (e) => {
        if (!dragState) return;
        e.preventDefault();
        const rows = Array.from(list.querySelectorAll('.sort-item-draggable'));
        for (const target of rows) {
            if (target === dragState.row) continue;
            const rect = target.getBoundingClientRect();
            if (e.clientY >= rect.top && e.clientY <= rect.bottom) {
                const mid = rect.top + rect.height / 2;
                if (e.clientY < mid) {
                    list.insertBefore(dragState.row, target);
                } else {
                    list.insertBefore(dragState.row, target.nextSibling);
                }
                break;
            }
        }
    });

    list.addEventListener('pointerup', (e) => {
        if (!dragState) return;
        e.preventDefault();
        dragState.row.classList.remove('dragging');

        const rows = Array.from(list.querySelectorAll('.sort-item-draggable'));
        const entryMap = {};
        entries.forEach(en => { entryMap[en.idx] = en; });
        const reordered = rows.map(r => entryMap[parseInt(r.dataset.gameIdx)]);

        let gi = 0;
        for (let pos = 0; pos < library.games.length; pos++) {
            if (library.games[pos].is_folder) {
                library.games[pos].sort[sortField] = pos;
            } else {
                reordered[gi].entry.sort[sortField] = pos;
                gi++;
            }
        }

        dragState = null;
        autoSave();
    });
}

function renderSortList(sortType) {
    const library = getLibrary();
    if (!library) return;
    // Clone to strip old event listeners (prevents accumulation on re-render)
    const oldList = document.getElementById('sort-list');
    const list = oldList.cloneNode(false);
    oldList.parentNode.replaceChild(list, oldList);

    // Only non-folder entries are sortable
    const entries = library.games
        .map((g, i) => ({ entry: g, idx: i }))
        .filter(e => !e.entry.is_folder);

    if (sortType === 'date') {
        // Sort by sor_date (current manual order)
        entries.sort((a, b) => a.entry.sort.sor_date - b.entry.sort.sor_date);
        entries.forEach(({ entry, idx }) => {
            const row = document.createElement('div');
            row.className = 'sort-item sort-item-draggable';
            row.dataset.gameIdx = idx;
            const handle = document.createElement('span');
            handle.className = 'sort-drag';
            handle.textContent = '\u2630';
            const name = document.createElement('span');
            name.className = 'sort-item-name';
            name.textContent = entry.game.display.name_eng || entry.game.display.name;
            row.appendChild(handle);
            row.appendChild(name);
            list.appendChild(row);
        });

        setupDragReorder(list, entries, library, 'sor_date');
    } else if (sortType === 'genre') {
        // Sort by genre, then name within genre
        entries.sort((a, b) => {
            const ga = a.entry.game.genre || 0;
            const gb = b.entry.game.genre || 0;
            if (ga && !gb) return -1;
            if (!ga && gb) return 1;
            if (ga !== gb) return ga - gb;
            const na = (a.entry.game.display.name_eng || '').toLowerCase();
            const nb = (b.entry.game.display.name_eng || '').toLowerCase();
            return na.localeCompare(nb);
        });
        let lastGenre = -1;
        entries.forEach(({ entry, idx }) => {
            const genre = entry.game.genre || 0;
            // Genre separator
            if (genre !== lastGenre) {
                const sep = document.createElement('div');
                sep.className = 'sort-genre-header';
                sep.textContent = genre ? genreNames[genre] : 'Unset';
                list.appendChild(sep);
                lastGenre = genre;
            }
            const row = document.createElement('div');
            row.className = 'sort-item';
            const name = document.createElement('span');
            name.className = 'sort-item-name';
            name.textContent = entry.game.display.name_eng || entry.game.display.name;
            const select = document.createElement('select');
            select.className = 'sort-item-select';
            genreNames.forEach((gn, gi) => {
                const opt = document.createElement('option');
                opt.value = gi;
                opt.textContent = gn;
                if (gi === genre) opt.selected = true;
                select.appendChild(opt);
            });
            select.addEventListener('change', () => {
                entry.game.genre = parseInt(select.value) || null;
                recomputeSortIndices(library.games);
                autoSave();
                renderSortList('genre');
            });
            row.appendChild(name);
            row.appendChild(select);
            list.appendChild(row);
        });
    } else if (sortType === 'demo') {
        // Sort by sor_demo (current manual order)
        entries.sort((a, b) => a.entry.sort.sor_demo - b.entry.sort.sor_demo);
        entries.forEach(({ entry, idx }) => {
            const row = document.createElement('div');
            row.className = 'sort-item sort-item-draggable';
            row.dataset.gameIdx = idx;
            const handle = document.createElement('span');
            handle.className = 'sort-drag';
            handle.textContent = '\u2630';
            const name = document.createElement('span');
            name.className = 'sort-item-name';
            name.textContent = entry.game.display.name_eng || entry.game.display.name;
            row.appendChild(handle);
            row.appendChild(name);
            list.appendChild(row);
        });

        setupDragReorder(list, entries, library, 'sor_demo');
    }
}

async function onSettingChange(key, value) {
    editorSettings[key] = value;
    await saveEditorSettingsToBackend();
}

// ---- Init ----
document.addEventListener('DOMContentLoaded', () => {
    document.getElementById('btn-open-folder').addEventListener('click', openFolder);
    document.getElementById('btn-add').addEventListener('click', addGame);
    document.getElementById('btn-add-folder').addEventListener('click', addFolder);
    document.getElementById('btn-delete').addEventListener('click', deleteGame);
    document.getElementById('btn-move').addEventListener('click', moveGame);
    document.getElementById('breadcrumb-back').addEventListener('click', exitFolder);
    document.getElementById('btn-rom-browse').addEventListener('click', pickRomFile);
    document.getElementById('btn-folder-cover').addEventListener('click', pickFolderCoverFromBreadcrumb);
    document.getElementById('folder-cover-drop').addEventListener('click', pickFolderCoverFromEditor);
    document.getElementById('btn-folder-mosaic-cover').addEventListener('click', openMosaicPicker);
    document.getElementById('mosaic-modal-cancel').addEventListener('click', closeMosaicPicker);
    document.getElementById('mosaic-modal-ok').addEventListener('click', buildMosaicCover);
    document.getElementById('btn-enter-folder').addEventListener('click', enterSelectedFolder);
    document.getElementById('f-folder-name').addEventListener('input', () => {
        const library = getLibrary();
        if (selectedIndex < 0 || !library) return;
        const entry = library.games[selectedIndex];
        if (!entry || !entry.is_folder) return;
        const val = document.getElementById('f-folder-name').value;
        entry.game.display.name = val;
        entry.game.display.name_eng = val;
        entry.game.display.tname = val;
        // Update header and list
        document.getElementById('folder-editor-title').textContent = val;
        const listItems = document.querySelectorAll('.game-item');
        if (listItems[selectedIndex]) {
            listItems[selectedIndex].querySelector('.game-item-name').textContent = val;
        }
        autoSave();
    });
    document.getElementById('btn-delete-folder').addEventListener('click', deleteGame);
    document.getElementById('btn-settings').addEventListener('click', openSettings);
    document.getElementById('btn-settings-close').addEventListener('click', closeSettings);
    document.getElementById('btn-change-folder').addEventListener('click', async () => {
        await openFolder();
        closeSettings();
    });
    document.getElementById('s-confirm-delete').addEventListener('change', (e) => {
        onSettingChange('confirm_delete', e.target.checked);
    });
    document.getElementById('s-force-us-titlebar').addEventListener('change', (e) => {
        onSettingChange('force_us_titlebar', e.target.checked);
        // Re-select current game to refresh titlebar state
        if (selectedIndex >= 0) {
            const library = getLibrary();
            if (library && !library.games[selectedIndex].is_folder) selectGame(selectedIndex);
        }
    });
    document.getElementById('btn-copy-name-to-jp').addEventListener('click', () => {
        const eng = document.getElementById('f-name-eng');
        const jp = document.getElementById('f-name');
        jp.value = eng.value;
        jp.dispatchEvent(new Event('input', { bubbles: true }));
    });
    document.getElementById('btn-add-genre').addEventListener('click', () => {
        const input = document.getElementById('s-genre-input');
        const name = input.value.trim();
        if (!name) return;
        if (!gamesSettings.genres) gamesSettings.genres = [];
        gamesSettings.genres.push(name);
        input.value = '';
        syncGenreNames();
        rebuildGenreDropdown();
        renderGenreSettings();
        saveGamesSettingsToBackend();
    });
    document.getElementById('s-genre-input').addEventListener('keydown', (e) => {
        if (e.key === 'Enter') document.getElementById('btn-add-genre').click();
    });
    document.getElementById('btn-add-alpha').addEventListener('click', () => {
        const input = document.getElementById('s-alpha-input');
        const prefix = input.value.trim();
        if (!prefix) return;
        if (!gamesSettings.alpha_exclusions) gamesSettings.alpha_exclusions = [];
        gamesSettings.alpha_exclusions.push(prefix);
        input.value = '';
        renderAlphaExclusionSettings();
        saveGamesSettingsToBackend();
        if (dualLibrary) {
            recomputeSortIndices(dualLibrary.jp.games);
            recomputeSortIndices(dualLibrary.us.games);
            autoSave();
        }
    });
    document.getElementById('s-alpha-input').addEventListener('keydown', (e) => {
        if (e.key === 'Enter') document.getElementById('btn-add-alpha').click();
    });
    document.getElementById('btn-sorting').addEventListener('click', openSortEditor);
    document.getElementById('btn-sorting-close').addEventListener('click', closeSortEditor);
    document.getElementById('btn-publish').addEventListener('click', publishLibrary);
    document.querySelectorAll('.sort-tab').forEach(tab => {
        tab.addEventListener('click', () => {
            document.querySelectorAll('.sort-tab').forEach(t => t.classList.remove('active'));
            tab.classList.add('active');
            renderSortList(tab.dataset.sort);
        });
    });
    initCcolorPicker();
    initTitlebarPicker();
    loadSettings();

    // Keyboard navigation: up/down arrows to switch games
    document.addEventListener('keydown', (e) => {
        if (e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT' || e.target.tagName === 'TEXTAREA') return;
        if (document.getElementById('modal-overlay').classList.contains('visible')) return;
        const library = getLibrary();
        if (!library || library.games.length === 0) return;
        let newIndex = -1;
        if (e.key === 'ArrowDown') {
            newIndex = selectedIndex < 0 ? 0 : Math.min(selectedIndex + 1, library.games.length - 1);
        } else if (e.key === 'ArrowUp') {
            newIndex = selectedIndex < 0 ? 0 : Math.max(selectedIndex - 1, 0);
        }
        if (newIndex >= 0 && newIndex !== selectedIndex) {
            e.preventDefault();
            const entry = library.games[newIndex];
            if (entry.is_folder) selectFolder(newIndex);
            else selectGame(newIndex);
        }
    });

    // Lineup tab switching
    document.querySelectorAll('.lineup-tab').forEach(tab => {
        tab.addEventListener('click', async () => {
            if (tab.dataset.lineup === currentLineup && !currentFolder) return;
            // Exit folder if inside one
            if (currentFolder) {
                // Update parent game_count
                const library = getLibrary();
                const gameCount = library.games.length;
                const parentEntry = currentFolder.parentGames.find(g => g.folder === currentFolder.name);
                if (parentEntry && parentEntry.game_count !== gameCount) {
                    parentEntry.game_count = gameCount;
                }
                restoreParentLibrary();
            }
            currentLineup = tab.dataset.lineup;
            document.querySelectorAll('.lineup-tab').forEach(t => t.classList.remove('active'));
            tab.classList.add('active');
            selectedIndex = -1;
            document.getElementById('editor').classList.remove('active');
            document.getElementById('folder-editor').style.display = 'none';
            document.getElementById('settings-panel').style.display = 'none';
            document.getElementById('placeholder').style.display = '';
            renderGameList();
            updateStats();
            // Auto-select first game
            const lib = getLibrary();
            if (lib && lib.games.length > 0) {
                if (lib.games[0].is_folder) selectFolder(0);
                else selectGame(0);
            }
        });
    });

    // Section collapse/expand
    document.querySelectorAll('.section-title').forEach(el => {
        el.addEventListener('click', () => el.parentElement.classList.toggle('open'));
    });

    // Preamp slider sync
    const slider = document.getElementById('f-preamp-slider');
    const num = document.getElementById('f-preamp');
    slider.addEventListener('input', () => { num.value = slider.value; onFieldChange(num); });
    num.addEventListener('input', () => { slider.value = num.value; });

    // Bind all form fields
    document.querySelectorAll('[data-path]').forEach(el => {
        el.addEventListener('input', () => onFieldChange(el));
        el.addEventListener('change', () => onFieldChange(el));
    });

    // Cover drop zones (click only — drag handled by Tauri onDragDropEvent below)
    document.getElementById('cover-drop').addEventListener('click', pickCoverFile);

    // Tauri native file drag-and-drop
    setupTauriDragDrop();
});

// ---- Open folder ----
async function openFolder() {
    try {
        const folder = await dialogOpen({ directory: true, title: 'Select library folder' });
        if (!folder) return;

        // Decide between "load existing" and "first-run init from NAND backup".
        const status = await invoke('check_library_status', { usbRoot: folder })
            .catch(e => { throw new Error('check_library_status: ' + e); });

        if (!status.has_library) {
            if (!status.has_backup) {
                modalAlert(
                    'No library and no BACKUP/game/ found.\n\n' +
                    'Either select a folder that contains a previously created library, ' +
                    'or place your NAND backup at <folder>/BACKUP/game/ and try again.'
                );
                return;
            }
            if (status.missing_templates && status.missing_templates.length) {
                modalAlert(
                    'BACKUP/game/ is missing required templates:\n  ' +
                    status.missing_templates.join('\n  ')
                );
                return;
            }
            const ok = await modalConfirm(
                'No library found. Create an empty library here from the BACKUP/game/ templates?'
            );
            if (!ok) return;
            try {
                const initRes = await invoke('init_library', { usbRoot: folder });
                console.log('[init_library]', initRes);
                // Run one publish so published/ is populated with empty packs
                // ready for the console mount.
                const pubRes = await invoke('publish_library', {
                    gamesPath: initRes.library_root,
                    stockDataRoot: '',
                    outputRoot: '',
                });
                console.log('[publish_library]', pubRes);
            } catch (e) {
                modalAlert('Library initialization failed: ' + e);
                return;
            }
        }

        await loadLibrary(folder);
        // Persist selected path
        onSettingChange('games_path', folder);
        updateGamesPathDisplay(folder);
    } catch (e) {
        modalAlert('Error opening folder: ' + e);
    }
}

async function loadLibrary(path) {
    try {
        // USB-style sync: backend resolves the library/published pair from
        // `path` automatically (wrapper layout: <picked>/library + <picked>/published;
        // legacy: <picked> itself as library + sibling published). Safe no-op
        // when published/ doesn't exist or is empty.
        try {
            const r = await invoke('sync_library', {
                libraryRoot: path,
                publishedRoot: '',
            });
            console.log('[sync_library]', r);
        } catch (e) {
            console.warn('sync_library failed (continuing):', e);
        }

        dualLibrary = await invoke('load_library', { gamesPath: path });
        await loadGamesSettings(path);
        coverCache = {};
        currentFolder = null;
        currentLineup = "jp";
        document.querySelectorAll('.lineup-tab').forEach(t => {
            t.classList.toggle('active', t.dataset.lineup === 'jp');
        });
        document.getElementById('sidebar').style.display = '';
        document.getElementById('lineup-tabs').style.display = '';
        document.getElementById('open-prompt').style.display = 'none';
        document.getElementById('placeholder').style.display = '';
        document.getElementById('editor').classList.remove('active');
        document.getElementById('folder-editor').style.display = 'none';
        selectedIndex = -1;
        renderGameList();
        updateStats();
        // Auto-select first game
        const library = getLibrary();
        if (library && library.games.length > 0) {
            if (library.games[0].is_folder) {
                selectFolder(0);
            } else {
                selectGame(0);
            }
        }
    } catch (e) {
        modalAlert('Error loading library: ' + e);
    }
}

function updateStats() {
    if (currentFolder) {
        const library = getLibrary();
        document.getElementById('library-stats').textContent =
            `${library.games.length} games in folder`;
    } else {
        const jpCount = dualLibrary.jp.games.length;
        const usCount = dualLibrary.us.games.length;
        document.getElementById('library-stats').textContent =
            `${jpCount} JP + ${usCount} US games`;
    }
}

// ---- Render game list ----
function renderGameList() {
    const library = getLibrary();
    const list = document.getElementById('game-list');
    list.innerHTML = '';

    // Breadcrumb visibility
    const breadcrumb = document.getElementById('folder-breadcrumb');
    const addFolderBtn = document.getElementById('btn-add-folder');
    if (currentFolder) {
        breadcrumb.style.display = '';
        document.getElementById('breadcrumb-folder-name').textContent =
            currentFolder.name;
        addFolderBtn.style.display = 'none';
    } else {
        breadcrumb.style.display = 'none';
        addFolderBtn.style.display = '';
    }

    if (!library) return;

    library.games.forEach((entry, i) => {
        const el = document.createElement('div');
        const isFolder = entry.is_folder;
        el.className = 'game-item' + (i === selectedIndex ? ' selected' : '') +
            (isFolder ? ' folder-item' : '');
        el.draggable = true;
        el.dataset.index = i;

        const img = document.createElement('img');
        img.className = 'game-item-cover';
        img.alt = '';
        if (isFolder) {
            loadFolderCoverThumb(entry, img);
        } else {
            loadCoverThumb(entry, img);
        }

        const info = document.createElement('div');
        info.className = 'game-item-info';
        if (isFolder) {
            info.innerHTML = `
                <div class="game-item-name">${escHtml(entry.game.display.name_eng || entry.game.display.name)}</div>
                <div class="game-item-meta">Folder · ${entry.game_count} games</div>
            `;
        } else {
            info.innerHTML = `
                <div class="game-item-name">${escHtml(entry.game.display.name_eng || entry.game.display.name)}</div>
                <div class="game-item-meta">${entry.game.rom.arch} · ${entry.game.rom.country}</div>
            `;
        }

        el.appendChild(img);
        el.appendChild(info);

        if (isFolder) {
            el.addEventListener('click', () => selectFolder(i));
            el.addEventListener('dblclick', () => enterFolder(entry));
        } else {
            el.addEventListener('click', () => selectGame(i));
        }

        // Drag and drop reordering
        el.addEventListener('dragstart', (e) => {
            e.dataTransfer.setData('text/plain', i.toString());
            el.classList.add('dragging');
        });
        el.addEventListener('dragend', () => el.classList.remove('dragging'));
        el.addEventListener('dragover', (e) => {
            e.preventDefault();
            el.classList.add('drag-over');
        });
        el.addEventListener('dragleave', () => el.classList.remove('drag-over'));
        el.addEventListener('drop', (e) => {
            e.preventDefault();
            el.classList.remove('drag-over');
            const from = parseInt(e.dataTransfer.getData('text/plain'));
            const to = i;
            if (from !== to) reorderGame(from, to);
        });

        list.appendChild(el);
    });
}

async function loadCoverThumb(entry, img) {
    const folderPrefix = currentFolder ? currentFolder.name + '/' : '';
    const cacheKey = currentLineup + '/' + folderPrefix + entry.folder;
    if (coverCache[cacheKey]) {
        img.src = coverCache[cacheKey];
        return;
    }
    try {
        const coverFile = entry.game.cover || 'cover.png';
        const lineup = currentFolder
            ? currentLineup + '/' + currentFolder.name
            : currentLineup;
        const data = await invoke('get_cover', {
            gamesPath: dualLibrary.path,
            lineup: lineup,
            folder: entry.folder,
            filename: coverFile
        });
        coverCache[cacheKey] = data;
        img.src = data;
    } catch {
        // No cover - leave blank
    }
}

async function loadFolderCoverThumb(entry, img) {
    const cacheKey = currentLineup + '/' + entry.folder;
    if (coverCache[cacheKey]) {
        img.src = coverCache[cacheKey];
        return;
    }
    try {
        const data = await invoke('get_cover', {
            gamesPath: dualLibrary.path,
            lineup: currentLineup,
            folder: entry.folder,
            filename: 'cover.png'
        });
        coverCache[cacheKey] = data;
        img.src = data;
    } catch {
        // No cover for folder - leave blank
    }
}

// ---- Folder navigation ----
async function enterFolder(entry) {
    try {
        const folderLib = await invoke('load_folder_contents', {
            gamesPath: dualLibrary.path,
            lineup: currentLineup,
            folderName: entry.folder
        });
        const library = getLibrary();
        currentFolder = {
            lineup: currentLineup,
            name: entry.folder,
            parentGames: library.games
        };
        library.games = folderLib.games;
        selectedIndex = -1;
        document.getElementById('editor').classList.remove('active');
        document.getElementById('folder-editor').style.display = 'none';
        document.getElementById('placeholder').style.display = '';
        renderGameList();
        updateStats();
    } catch (e) {
        modalAlert('Error opening folder: ' + e);
    }
}

async function exitFolder() {
    if (!currentFolder) return;
    // Update parent folder's game_count
    const library = getLibrary();
    const gameCount = library.games.length;
    const parentEntry = currentFolder.parentGames.find(g => g.folder === currentFolder.name);
    if (parentEntry && parentEntry.game_count !== gameCount) {
        parentEntry.game_count = gameCount;
    }
    restoreParentLibrary();
    selectedIndex = -1;
    document.getElementById('editor').classList.remove('active');
    document.getElementById('folder-editor').style.display = 'none';
    document.getElementById('placeholder').style.display = '';
    renderGameList();
    updateStats();
}

function restoreParentLibrary() {
    if (!currentFolder) return;
    const library = dualLibrary[currentFolder.lineup];
    library.games = currentFolder.parentGames;
    currentFolder = null;
}

// ---- Select folder (single click) ----
function selectFolder(index) {
    const library = getLibrary();
    selectedIndex = index;
    const entry = library.games[index];

    document.getElementById('placeholder').style.display = 'none';
    document.getElementById('editor').classList.remove('active');
    document.getElementById('settings-panel').style.display = 'none';
    document.getElementById('sorting-panel').style.display = 'none';
    document.getElementById('folder-editor').style.display = 'block';

    // Title
    document.getElementById('folder-editor-title').textContent =
        entry.game.display.name_eng || entry.game.display.name;
    document.getElementById('folder-editor-subtitle').textContent =
        entry.folder + ' \u00B7 ' + entry.game_count + ' games';

    // Display name field
    document.getElementById('f-folder-name').value =
        entry.game.display.name_eng || entry.game.display.name;

    // Cover
    const cacheKey = currentLineup + '/' + entry.folder;
    const coverImg = document.getElementById('folder-editor-cover');
    const coverPreview = document.getElementById('folder-cover-preview');
    if (coverCache[cacheKey]) {
        coverImg.src = coverCache[cacheKey];
        coverPreview.src = coverCache[cacheKey];
    } else {
        coverImg.src = '';
        coverPreview.src = '';
        loadFolderCoverThumb(entry, coverImg).then(() => {
            coverPreview.src = coverImg.src;
        });
    }

    // Highlight in list
    document.querySelectorAll('.game-item').forEach((el, i) => {
        el.classList.toggle('selected', i === index);
    });
}

async function enterSelectedFolder() {
    const library = getLibrary();
    if (selectedIndex < 0) return;
    const entry = library.games[selectedIndex];
    if (!entry || !entry.is_folder) return;
    document.getElementById('folder-editor').style.display = 'none';
    await enterFolder(entry);
}

// ---- Select game ----
function selectGame(index) {
    const library = getLibrary();
    selectedIndex = index;
    const entry = library.games[index];

    document.getElementById('placeholder').style.display = 'none';
    document.getElementById('folder-editor').style.display = 'none';
    document.getElementById('settings-panel').style.display = 'none';
    document.getElementById('sorting-panel').style.display = 'none';
    document.getElementById('editor').classList.add('active');

    // Update header
    document.getElementById('editor-title').textContent =
        entry.game.display.name_eng || entry.game.display.name;
    document.getElementById('editor-subtitle').textContent = entry.folder;

    const tags = document.getElementById('editor-tags');
    tags.innerHTML = `
        <span class="tag tag-arch">${entry.game.rom.arch}</span>
        <span class="tag tag-country">${entry.game.rom.country}</span>
        <span class="tag tag-players">${entry.game.display.players}P</span>
    `;

    // Cover
    const folderPrefix = currentFolder ? currentFolder.name + '/' : '';
    const cacheKey = currentLineup + '/' + folderPrefix + entry.folder;
    const coverImg = document.getElementById('editor-cover');
    const coverPreview = document.getElementById('cover-preview');
    if (coverCache[cacheKey]) {
        coverImg.src = coverCache[cacheKey];
        coverPreview.src = coverCache[cacheKey];
    } else {
        coverImg.src = '';
        coverPreview.src = '';
        loadCoverThumb(entry, coverImg).then(() => {
            coverPreview.src = coverImg.src;
            // Update ccolor thumbnails once cover loads
            for (let i = 0; i <= 2; i++) {
                const thumb = document.getElementById('ccolor-thumb-' + i);
                if (thumb) thumb.src = coverImg.src;
            }
        });
    }

    // Update cover box background based on ccolor
    updateCoverBoxColor(entry.game.display.ccolor);

    // Update ccolor thumbnails
    const coverSrc = coverCache[cacheKey] || '';
    for (let i = 0; i <= 2; i++) {
        const thumb = document.getElementById('ccolor-thumb-' + i);
        if (thumb) thumb.src = coverSrc;
    }

    // Populate form fields
    setField('f-name', entry.game.display.name);
    setField('f-name-eng', entry.game.display.name_eng);
    setField('f-players', entry.game.display.players);
    setField('f-titlebar', entry.game.display.titlebar);
    updateTitlebarSelection(entry.game.display.titlebar);
    applyForcedTitlebar();
    setField('f-ccolor', entry.game.display.ccolor);
    setField('f-csize', entry.game.display.csize);
    setField('f-demo-time', entry.game.display.demo_time);
    setField('f-genre', entry.game.genre || 0);

    setField('f-rom', entry.game.rom.rom);
    setField('f-preamp', entry.game.rom.preamp);
    document.getElementById('f-preamp-slider').value = entry.game.rom.preamp;



    // Populate ROM datalist from game folder
    populateRomDatalist(entry);

    // Load save states
    loadSaveStates(entry);

    // Highlight in list
    document.querySelectorAll('.game-item').forEach((el, i) => {
        el.classList.toggle('selected', i === index);
    });
}

function setField(id, value) {
    const el = document.getElementById(id);
    if (el) el.value = value ?? '';
}

// ---- Field change handler ----
function onFieldChange(el) {
    const library = getLibrary();
    if (selectedIndex < 0 || !library) return;
    const path = el.dataset.path;
    if (!path) return;

    const entry = library.games[selectedIndex];
    const parts = path.split('.');
    let obj = entry.game;

    for (let i = 0; i < parts.length - 1; i++) {
        if (!obj[parts[i]]) obj[parts[i]] = {};
        obj = obj[parts[i]];
    }

    const key = parts[parts.length - 1];
    let val = el.value;

    // Type coercion
    if (el.type === 'number') {
        val = el.value === '' ? 0 : parseFloat(el.value);
        if (Number.isInteger(val) && !path.includes('preamp')) val = Math.round(val);
    } else if (el.tagName === 'SELECT' && !isNaN(parseInt(val))) {
        val = parseInt(val);
    }

    obj[key] = val;
    autoSave();

    // Keep tname in sync with name (tname is the display name used by the emu)
    if (path === 'display.name') {
        entry.game.display.tname = val;
    }

    // Update header live
    if (path === 'display.name_eng' || path === 'display.name') {
        const name = entry.game.display.name_eng || entry.game.display.name;
        document.getElementById('editor-title').textContent = name;
        const listItems = document.querySelectorAll('.game-item');
        if (listItems[selectedIndex]) {
            listItems[selectedIndex].querySelector('.game-item-name').textContent = name;
        }
    }
    if (path === 'display.players') {
        updateTags(entry);
        recomputeSortIndices(library.games);
    }
    // Recompute sorts when name or genre changes
    if (path === 'display.name_eng' || path === 'genre') {
        recomputeSortIndices(library.games);
    }
    // Auto-set arch from platform (csize)
    if (path === 'display.csize') {
        syncArchFromPlatform(entry);
        updateTags(entry);
        applyForcedTitlebar();
    }
}

// csize → rom.arch mapping
const PLATFORM_TO_ARCH = { 0: 'tg16', 1: 'sgx', 2: 'tg16cd', 3: 'tg16cd', 4: 'tg16cd' };
const PLATFORM_LABELS = { 0: 'HuCard', 1: 'SuperGrafx', 2: 'CD-ROM2', 3: 'Super CD-ROM2', 4: 'Arcade CD-ROM2' };

function syncArchFromPlatform(entry) {
    const csize = entry.game.display.csize;
    entry.game.rom.arch = PLATFORM_TO_ARCH[csize] || 'tg16';
    // Auto-set system card for CD platforms
    if (csize === 3) entry.game.rom.tg16cd_systemcard = 'super';
    else if (csize === 4) entry.game.rom.tg16cd_systemcard = 'arcade';
    else entry.game.rom.tg16cd_systemcard = null;
    // Country always matches lineup
    entry.game.rom.country = currentLineup;
}

function updateTags(entry) {
    const platform = PLATFORM_LABELS[entry.game.display.csize] || entry.game.rom.arch;
    document.getElementById('editor-tags').innerHTML = `
        <span class="tag tag-arch">${platform}</span>
        <span class="tag tag-country">${entry.game.rom.country}</span>
        <span class="tag tag-players">${entry.game.display.players}P</span>
    `;
}

const BOX_FRAMES = { 0: 'box_white.png', 1: 'box_black.png', 2: 'box_wide.png' };

// Cover dimensions per ccolor (must match frame interior proportions at 3x)
// Interior pixels: black=81x80, white=81x79, wide=94x80 → at 3x scale
const COVER_DIMS = {
    0: { width: 240, height: 240 },  // Black case
    1: { width: 240, height: 240 },  // White case
    2: { width: 280, height: 240 },  // Double CD (fat case)
};

// CSS positioning for cover inside each box frame (% of frame size)
const COVER_POS = {
    0: { left: '9.2%', top: '2.4%', width: '82.7%', height: '95.2%' },
    1: { left: '9.2%', top: '3.6%', width: '82.7%', height: '94.0%' },
    2: { left: '2.0%', top: '2.4%', width: '95.9%', height: '95.2%' },
};

function getCoverDims(ccolor) {
    return COVER_DIMS[ccolor] || COVER_DIMS[0];
}

function updateCoverBoxColor(ccolor) {
    // Update box frame overlay image
    const frame = document.getElementById('box-frame');
    frame.style.backgroundImage = `url(${BOX_FRAMES[ccolor] || 'box_white.png'})`;

    // Update cover positioning inside the box
    const pos = COVER_POS[ccolor] || COVER_POS[0];
    const cover = document.getElementById('editor-cover');
    cover.style.left = pos.left;
    cover.style.top = pos.top;
    cover.style.width = pos.width;
    cover.style.height = pos.height;

    // Update ccolor picker selection
    document.querySelectorAll('.ccolor-option').forEach(opt => {
        opt.classList.toggle('selected', parseInt(opt.dataset.ccolor) === ccolor);
    });

    // Update hidden input
    document.getElementById('f-ccolor').value = ccolor;
}

function initCcolorPicker() {
    document.querySelectorAll('.ccolor-option').forEach(opt => {
        opt.addEventListener('click', () => {
            const library = getLibrary();
            if (selectedIndex < 0 || !library) return;
            const entry = library.games[selectedIndex];
            const val = parseInt(opt.dataset.ccolor);
            entry.game.display.ccolor = val;
            // Update cover_size based on box type
            const dims = getCoverDims(val);
            if (dims.width !== 240 || dims.height !== 240) {
                entry.game.cover_size = { width: dims.width, height: dims.height, originX: Math.floor(dims.width / 2), originY: Math.floor(dims.height / 2) };
            } else {
                entry.game.cover_size = null;
            }
            updateCoverBoxColor(val);
            autoSave();
        });
    });
}

// Returns the forced titlebar value for US lineup games, or null if not forced.
// US HuCard (csize 0,1) → titlebar 9 (TG16), US CD-ROM (csize 2,3,4) → titlebar 10 (TG16-CD)
function getForcedTitlebar() {
    if (!editorSettings.force_us_titlebar || currentLineup !== 'us') return null;
    if (selectedIndex < 0) return null;
    const library = getLibrary();
    if (!library) return null;
    const entry = library.games[selectedIndex];
    if (!entry || entry.is_folder) return null;
    const csize = entry.game.display.csize;
    return (csize <= 1) ? 9 : 10;
}

function applyForcedTitlebar() {
    const forced = getForcedTitlebar();
    const picker = document.getElementById('titlebar-picker');
    if (forced !== null) {
        const library = getLibrary();
        const entry = library.games[selectedIndex];
        entry.game.display.titlebar = forced;
        document.getElementById('f-titlebar').value = forced;
        updateTitlebarSelection(forced);
        picker.classList.add('disabled');
    } else {
        picker.classList.remove('disabled');
    }
}

function initTitlebarPicker() {
    const container = document.getElementById('titlebar-picker');
    for (let i = 1; i <= 12; i++) {
        const opt = document.createElement('div');
        opt.className = 'titlebar-option';
        opt.dataset.titlebar = i;
        opt.title = 'Titlebar ' + i;
        const img = document.createElement('img');
        img.src = 'titlebars/button_title_' + i + '.png';
        img.alt = '' + i;
        opt.appendChild(img);
        opt.addEventListener('click', () => {
            if (getForcedTitlebar() !== null) return;
            const library = getLibrary();
            if (selectedIndex < 0 || !library) return;
            library.games[selectedIndex].game.display.titlebar = i;
            document.getElementById('f-titlebar').value = i;
            updateTitlebarSelection(i);
            autoSave();
        });
        container.appendChild(opt);
    }
}

function updateTitlebarSelection(val) {
    document.querySelectorAll('.titlebar-option').forEach(opt => {
        opt.classList.toggle('selected', parseInt(opt.dataset.titlebar) === val);
    });
}

// ---- Cover ----
async function pickCoverFile() {
    try {
        const file = await dialogOpen({
            title: 'Select cover image',
            filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'bmp', 'gif', 'webp', 'tiff', 'tif'] }]
        });
        if (!file) return;
        await importCover(file);
    } catch (e) {
        console.error('Cover pick error:', e);
    }
}

// Tauri native drag-and-drop: routes dropped image files to the visible cover zone
function setupTauriDragDrop() {
    const coverDrop = document.getElementById('cover-drop');
    const folderCoverDrop = document.getElementById('folder-cover-drop');
    const imageExts = ['png', 'jpg', 'jpeg', 'bmp', 'gif', 'webp', 'tiff', 'tif'];

    getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === 'enter' || event.payload.type === 'over') {
            // Highlight the visible drop zone
            if (coverDrop.offsetParent !== null) coverDrop.classList.add('dragover');
            if (folderCoverDrop.offsetParent !== null) folderCoverDrop.classList.add('dragover');
        } else if (event.payload.type === 'leave') {
            coverDrop.classList.remove('dragover');
            folderCoverDrop.classList.remove('dragover');
        } else if (event.payload.type === 'drop') {
            coverDrop.classList.remove('dragover');
            folderCoverDrop.classList.remove('dragover');
            const paths = event.payload.paths;
            if (!paths || paths.length === 0) return;
            // Use first image file from the drop
            const file = paths.find(p => {
                const ext = p.split('.').pop().toLowerCase();
                return imageExts.includes(ext);
            });
            if (!file) return;
            // Route to the visible cover zone
            if (folderCoverDrop.offsetParent !== null) {
                handleFolderCoverDrop(file);
            } else if (coverDrop.offsetParent !== null) {
                importCover(file);
            }
        }
    });
}

// Handle a file dropped on the folder cover zone
async function handleFolderCoverDrop(filePath) {
    const library = getLibrary();
    if (selectedIndex < 0 || !library) return;
    const entry = library.games[selectedIndex];
    if (!entry || !entry.is_folder) return;
    const data = await importFolderCover(entry.folder, filePath);
    if (data) {
        document.getElementById('folder-editor-cover').src = data;
        document.getElementById('folder-cover-preview').src = data;
        const listImg = document.querySelectorAll('.game-item')[selectedIndex]?.querySelector('img');
        if (listImg) listImg.src = data;
    }
}

async function importCover(srcPath) {
    const library = getLibrary();
    const entry = library.games[selectedIndex];
    const coverName = entry.game.cover || 'cover.png';
    const folderPrefix = currentFolder ? currentFolder.name + '/' : '';
    const destDir = dualLibrary.path + '/' + currentLineup + '/' + folderPrefix + entry.folder;

    // Derive cover dimensions from ccolor
    const dims = getCoverDims(entry.game.display.ccolor);
    const w = dims.width;
    const h = dims.height;

    try {
        await invoke('import_cover', { srcPath, destDir, filename: coverName, width: w, height: h });
        // Reload cover
        const cacheKey = currentLineup + '/' + folderPrefix + entry.folder;
        delete coverCache[cacheKey];
        const lineup = currentFolder
            ? currentLineup + '/' + currentFolder.name
            : currentLineup;
        const data = await invoke('get_cover', {
            gamesPath: dualLibrary.path,
            lineup: lineup,
            folder: entry.folder,
            filename: coverName
        });
        coverCache[cacheKey] = data;
        document.getElementById('editor-cover').src = data;
        document.getElementById('cover-preview').src = data;
        // Update ccolor thumbnails
        for (let i = 0; i <= 2; i++) {
            const thumb = document.getElementById('ccolor-thumb-' + i);
            if (thumb) thumb.src = data;
        }
        // Update list thumbnail
        const listImg = document.querySelectorAll('.game-item')[selectedIndex]?.querySelector('img');
        if (listImg) listImg.src = data;
        entry.has_cover = true;
    } catch (e) {
        modalAlert('Error importing cover: ' + e);
    }
}

// ---- ROM file import ----
async function pickRomFile() {
    if (selectedIndex < 0) return;
    try {
        const file = await dialogOpen({
            title: 'Select ROM file',
            filters: [{ name: 'ROM files', extensions: ['pce', 'PCE', 'pcd', 'PCD', 'bin'] }]
        });
        if (!file) return;
        await importRomFile(file, 'rom.rom', document.getElementById('f-rom'));
    } catch (e) {
        console.error('ROM pick error:', e);
    }
}

async function importRomFile(srcPath, dataPath, inputEl) {
    const library = getLibrary();
    const entry = library.games[selectedIndex];
    const filename = srcPath.split('/').pop().split('\\').pop();
    const folderPrefix = currentFolder ? currentFolder.name + '/' : '';
    const destDir = dualLibrary.path + '/' + currentLineup + '/' + folderPrefix + entry.folder;

    try {
        await invoke('import_file', { srcPath, destDir, filename });
        // Update the rom field to just the filename
        const parts = dataPath.split('.');
        let obj = entry.game;
        for (let i = 0; i < parts.length - 1; i++) obj = obj[parts[i]];
        obj[parts[parts.length - 1]] = filename;
        inputEl.value = filename;
        autoSave();
        // Refresh ROM datalist
        await populateRomDatalist(entry);
    } catch (e) {
        modalAlert('Error importing ROM: ' + e);
    }
}

async function populateRomDatalist(entry) {
    const datalist = document.getElementById('rom-datalist');
    datalist.innerHTML = '';
    const folderPrefix = currentFolder ? currentFolder.name + '/' : '';
    const folderPath = dualLibrary.path + '/' + currentLineup + '/' + folderPrefix + entry.folder;
    try {
        const files = await invoke('list_files_in_folder', {
            folderPath,
            extensions: ['pce', 'PCE', 'pcd', 'PCD', 'bin']
        });
        for (const f of files) {
            const opt = document.createElement('option');
            opt.value = f;
            datalist.appendChild(opt);
        }
    } catch {
        // Folder may not exist yet
    }
}

// ---- Folder cover import ----
async function importFolderCover(folderName, srcPath) {
    const destDir = dualLibrary.path + '/' + currentLineup + '/' + folderName;
    try {
        await invoke('import_cover', { srcPath, destDir, filename: 'cover.png', width: 240, height: 240 });
        // Clear and reload cache
        const cacheKey = currentLineup + '/' + folderName;
        delete coverCache[cacheKey];
        const data = await invoke('get_cover', {
            gamesPath: dualLibrary.path,
            lineup: currentLineup,
            folder: folderName,
            filename: 'cover.png'
        });
        coverCache[cacheKey] = data;
        return data;
    } catch (e) {
        modalAlert('Error setting folder cover: ' + e);
        return null;
    }
}

// From breadcrumb (inside folder)
async function pickFolderCoverFromBreadcrumb() {
    if (!currentFolder) return;
    try {
        const file = await dialogOpen({
            title: 'Select folder cover image',
            filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'bmp', 'gif', 'webp', 'tiff', 'tif'] }]
        });
        if (!file) return;
        await importFolderCover(currentFolder.name, file);
    } catch (e) {
        console.error('Folder cover pick error:', e);
    }
}

// From folder editor panel (parent level)
async function pickFolderCoverFromEditor() {
    const library = getLibrary();
    if (selectedIndex < 0 || !library) return;
    const entry = library.games[selectedIndex];
    if (!entry || !entry.is_folder) return;
    try {
        const file = await dialogOpen({
            title: 'Select folder cover image',
            filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'bmp', 'gif', 'webp', 'tiff', 'tif'] }]
        });
        if (!file) return;
        const data = await importFolderCover(entry.folder, file);
        if (data) {
            document.getElementById('folder-editor-cover').src = data;
            document.getElementById('folder-cover-preview').src = data;
            // Update list thumbnail
            const listImg = document.querySelectorAll('.game-item')[selectedIndex]?.querySelector('img');
            if (listImg) listImg.src = data;
        }
    } catch (e) {
        console.error('Folder cover pick error:', e);
    }
}

// ---- Mosaic folder cover (2x2 of game covers) ----

// Ordered list of game dir_names currently selected, [TL, TR, BL, BR].
// Null entries are empty slots until 4 are picked.
let mosaicSelection = [];
// The folder entry whose mosaic we're building (cached when modal opens).
let mosaicFolderEntry = null;

async function openMosaicPicker() {
    const library = getLibrary();
    if (selectedIndex < 0 || !library) return;
    const entry = library.games[selectedIndex];
    if (!entry || !entry.is_folder) return;

    let folderLib;
    try {
        folderLib = await invoke('load_folder_contents', {
            gamesPath: dualLibrary.path,
            lineup: currentLineup,
            folderName: entry.folder,
        });
    } catch (e) {
        modalAlert('Failed to load folder contents: ' + e);
        return;
    }

    const realGames = (folderLib.games || []).filter(g => !g.is_folder);
    if (realGames.length < 4) {
        modalAlert(`Folder has ${realGames.length} game(s); need at least 4 for a mosaic.`);
        return;
    }

    mosaicFolderEntry = entry;
    mosaicSelection = [];

    const grid = document.getElementById('mosaic-game-grid');
    grid.innerHTML = '';
    for (const g of realGames) {
        const tile = document.createElement('div');
        tile.className = 'mosaic-tile';
        tile.dataset.folder = g.folder;
        tile.style.cssText = 'position:relative; cursor:pointer; border:2px solid transparent; border-radius:6px; padding:4px; text-align:center; font-size:11px';
        const img = document.createElement('img');
        img.style.cssText = 'width:100px; height:100px; object-fit:cover; display:block; margin:0 auto 4px';
        img.alt = g.folder;
        // Async load cover. Games are inside `mosaicFolderEntry.folder`,
        // so lineup arg is `<lineup>/<folder>` per get_cover convention.
        invoke('get_cover', {
            gamesPath: dualLibrary.path,
            lineup: currentLineup + '/' + entry.folder,
            folder: g.folder,
            filename: g.game.cover || 'cover.png',
        }).then(data => { if (data) img.src = data; }).catch(() => {});
        const label = document.createElement('div');
        label.textContent = g.game.display.name_eng || g.game.display.name || g.folder;
        label.style.cssText = 'overflow:hidden; text-overflow:ellipsis; white-space:nowrap';
        const badge = document.createElement('div');
        badge.className = 'mosaic-tile-badge';
        badge.style.cssText = 'position:absolute; top:6px; right:6px; width:20px; height:20px; border-radius:50%; background:#4a90e2; color:#fff; font-weight:bold; line-height:20px; display:none';
        tile.appendChild(img);
        tile.appendChild(label);
        tile.appendChild(badge);
        tile.addEventListener('click', () => toggleMosaicSelection(g.folder));
        grid.appendChild(tile);
    }

    updateMosaicPickerUI();
    document.getElementById('mosaic-modal-overlay').classList.add('visible');
}

function toggleMosaicSelection(folder) {
    const idx = mosaicSelection.indexOf(folder);
    if (idx >= 0) {
        mosaicSelection.splice(idx, 1);
    } else {
        if (mosaicSelection.length >= 4) return; // cap at 4
        mosaicSelection.push(folder);
    }
    updateMosaicPickerUI();
}

function updateMosaicPickerUI() {
    const grid = document.getElementById('mosaic-game-grid');
    grid.querySelectorAll('.mosaic-tile').forEach(tile => {
        const folder = tile.dataset.folder;
        const order = mosaicSelection.indexOf(folder);
        const badge = tile.querySelector('.mosaic-tile-badge');
        if (order >= 0) {
            tile.style.borderColor = '#4a90e2';
            badge.textContent = String(order + 1);
            badge.style.display = 'block';
        } else {
            tile.style.borderColor = 'transparent';
            badge.style.display = 'none';
        }
    });
    const slots = ['TL', 'TR', 'BL', 'BR'];
    const labels = mosaicSelection.map((f, i) => `${slots[i]}: ${f}`).join('   ');
    document.getElementById('mosaic-modal-hint').textContent =
        mosaicSelection.length === 4
            ? labels
            : `Pick ${4 - mosaicSelection.length} more game(s). Order: top-left, top-right, bottom-left, bottom-right.`;
    document.getElementById('mosaic-modal-ok').disabled = mosaicSelection.length !== 4;
}

function closeMosaicPicker() {
    document.getElementById('mosaic-modal-overlay').classList.remove('visible');
    mosaicFolderEntry = null;
    mosaicSelection = [];
}

async function buildMosaicCover() {
    if (!mosaicFolderEntry || mosaicSelection.length !== 4) return;
    const folderDir = dualLibrary.path + '/' + currentLineup + '/' + mosaicFolderEntry.folder;
    try {
        await invoke('generate_folder_mosaic_cover', {
            folderDir,
            gameDirNames: [...mosaicSelection],
        });
    } catch (e) {
        modalAlert('Failed to build mosaic: ' + e);
        return;
    }
    // Refresh cover in editor + list. Bust cache.
    const cacheKey = currentLineup + '/' + mosaicFolderEntry.folder;
    delete coverCache[cacheKey];
    try {
        const data = await invoke('get_cover', {
            gamesPath: dualLibrary.path,
            lineup: currentLineup,
            folder: mosaicFolderEntry.folder,
            filename: 'cover.png',
        });
        if (data) {
            coverCache[cacheKey] = data;
            document.getElementById('folder-editor-cover').src = data;
            document.getElementById('folder-cover-preview').src = data;
            const listImg = document.querySelectorAll('.game-item')[selectedIndex]?.querySelector('img');
            if (listImg) listImg.src = data;
        }
    } catch (_e) {}
    closeMosaicPicker();
}

// ---- Save States ----
async function loadSaveStates(entry) {
    const grid = document.getElementById('save-states-grid');
    grid.innerHTML = '';

    const library = getLibrary();
    const folderPrefix = currentFolder ? currentFolder.name + '/' : '';
    const gamePath = library.path + '/' + folderPrefix + entry.folder;

    let states = [];
    try {
        states = await invoke('get_save_states', { gamePath });
    } catch (e) {
        // No save states or error
    }

    for (const state of states) {
        const slotDiv = document.createElement('div');
        slotDiv.className = 'save-state-slot';

        if (state.exists && state.thumbnail) {
            const img = document.createElement('img');
            img.className = 'save-state-thumb';
            img.src = state.thumbnail;
            img.alt = 'Slot ' + (state.slot + 1);
            slotDiv.appendChild(img);
        } else {
            const empty = document.createElement('div');
            empty.className = 'save-state-empty';
            empty.textContent = state.exists ? 'No preview' : 'Empty';
            slotDiv.appendChild(empty);
        }

        const label = document.createElement('div');
        label.className = 'save-state-label';
        label.textContent = 'Slot ' + (state.slot + 1);
        slotDiv.appendChild(label);
        grid.appendChild(slotDiv);
    }
}

// ---- Sort index normalization ----

// Recompute all sort indices from game metadata.
// sor_name: alphabetical by name_eng (auto)
// sor_pnum: by player count, then name (auto)
// sor_genr: by genre (nulls last), then name within genre (auto)
// sor_date: preserved as-is (manual order), normalized to 0..N-1
// sor_demo: preserved as-is (manual order), normalized to 0..N-1
function recomputeSortIndices(games) {
    const n = games.length;
    if (n === 0) return;

    // Helper: sort non-folder entries by a comparator, assign 0..N-1.
    // Folders keep their gamelist position in every sort field.
    function assignField(field, cmp) {
        const indexed = games.map((g, i) => ({ i, g }));
        const nonFolders = indexed.filter(e => !e.g.is_folder);

        nonFolders.sort((a, b) => cmp(a.g, b.g));

        let gi = 0;
        for (let pos = 0; pos < n; pos++) {
            if (games[pos].is_folder) {
                games[pos].sort[field] = pos;
            } else {
                nonFolders[gi].g.sort[field] = pos;
                gi++;
            }
        }
    }

    // Helper: normalize existing values to 0..N-1 preserving relative order
    function normalizeField(field) {
        const indexed = games.map((g, i) => ({ i, val: g.sort[field] }));
        indexed.sort((a, b) => a.val - b.val);
        indexed.forEach((entry, rank) => {
            games[entry.i].sort[field] = rank;
        });
    }

    const nameOf = g => (g.game.display.name_eng || g.game.display.name || '').toLowerCase();

    function sortableName(g) {
        let name = nameOf(g);
        for (const prefix of gamesSettings.alpha_exclusions || []) {
            const p = prefix.toLowerCase() + ' ';
            if (name.startsWith(p)) {
                name = name.slice(p.length);
                break;
            }
        }
        return name;
    }

    // sor_name: alphabetical (with prefix exclusions)
    assignField('sor_name', (a, b) => sortableName(a).localeCompare(sortableName(b)));

    // sor_pnum: by player count, then name
    assignField('sor_pnum', (a, b) => {
        const d = (a.game.display.players || 1) - (b.game.display.players || 1);
        return d !== 0 ? d : nameOf(a).localeCompare(nameOf(b));
    });

    // sor_genr: by genre (0/null last), then name within genre
    assignField('sor_genr', (a, b) => {
        const ga = a.game.genre || 0;
        const gb = b.game.genre || 0;
        if (ga && !gb) return -1;
        if (!ga && gb) return 1;
        if (ga !== gb) return ga - gb;
        return nameOf(a).localeCompare(nameOf(b));
    });

    // sor_date & sor_demo: manual order, just normalize to 0..N-1
    normalizeField('sor_date');
    normalizeField('sor_demo');
}

// ---- Reorder ----
function reorderGame(from, to) {
    const library = getLibrary();
    const [item] = library.games.splice(from, 1);
    library.games.splice(to, 0, item);

    // sor_demo follows list order when reordering via drag-and-drop
    library.games.forEach((g, i) => {
        g.sort.sor_demo = i;
    });
    recomputeSortIndices(library.games);

    if (selectedIndex === from) selectedIndex = to;
    else if (from < selectedIndex && to >= selectedIndex) selectedIndex--;
    else if (from > selectedIndex && to <= selectedIndex) selectedIndex++;

    autoSave();
    renderGameList();
}

// ---- Add game ----
async function addGame() {
    // Generate a unique folder name: GAME00, GAME01, ...
    const library = getLibrary();
    const lineupForCreate = currentFolder
        ? currentLineup + '/' + currentFolder.name
        : currentLineup;
    const existingFolders = new Set(library.games.map(g => g.folder));
    let name;
    for (let n = 0; n < 100; n++) {
        name = 'GAME' + String(n).padStart(2, '0');
        if (!existingFolders.has(name)) break;
    }

    try {
        const entry = await invoke('create_game', {
            gamesPath: dualLibrary.path,
            lineup: lineupForCreate,
            folderName: name
        });
        // Set sort indices based on position
        entry.sort.sor_demo = library.games.length;
        entry.sort.sor_pnum = library.games.length;
        entry.sort.sor_date = library.games.length;
        entry.sort.sor_name = library.games.length;
        entry.sort.sor_genr = library.games.length;
        library.games.push(entry);
        autoSave();
        renderGameList();
        selectGame(library.games.length - 1);
        updateStats();
    } catch (e) {
        modalAlert('Error creating game: ' + e);
    }
}

// ---- Add folder ----
async function addFolder() {
    const name = await modalPrompt('Folder display name', 'e.g. Shooters, RPGs, Favorites...');
    if (!name || !name.trim()) return;
    const displayName = name.trim();

    // Generate unique folder name: FOLDER00, FOLDER01, ...
    const library = getLibrary();
    const existingFolders = new Set(library.games.map(g => g.folder));
    let folderName;
    for (let n = 0; n < 100; n++) {
        folderName = 'FOLDER' + String(n).padStart(2, '0');
        if (!existingFolders.has(folderName)) break;
    }

    try {
        await invoke('create_folder', {
            gamesPath: dualLibrary.path,
            lineup: currentLineup,
            folderName: folderName
        });

        const sort = {
            folder: folderName,
            sor_date: library.games.length,
            sor_demo: library.games.length,
            sor_genr: library.games.length,
            sor_name: library.games.length,
            sor_pnum: library.games.length
        };
        library.games.push({
            folder: folderName,
            game: {
                display: { name: displayName, tname: displayName, name_eng: displayName, titlebar: 4, ccolor: 0, csize: 0, players: 0, demo_time: 0 },
                rom: { arch: 'folder', country: '', preamp: 0, rom: '' },
                cover: 'cover.png',
                cover_size: null
            },
            has_cover: false,
            sort: sort,
            is_folder: true,
            game_count: 0
        });
        autoSave();
        renderGameList();
        updateStats();
    } catch (e) {
        modalAlert('Error creating folder: ' + e);
    }
}

// ---- Move game between lineups/folders ----
// Presents a destination picker (other lineup, subfolders) via modalSelect,
// moves the game folder on disk, updates both source and destination gamelists,
// and applies forced titlebar if moving to US lineup.
async function moveGame() {
    const library = getLibrary();
    if (selectedIndex < 0) return;
    const entry = library.games[selectedIndex];
    if (entry.is_folder) return; // folders can't be moved (yet)

    // Build destination options
    const options = [];
    const otherLineup = currentLineup === 'jp' ? 'us' : 'jp';
    const otherLabel = otherLineup.toUpperCase();

    if (currentFolder) {
        // Inside a folder: offer to move to top level of same lineup
        options.push({ label: `Top level (${currentLineup.toUpperCase()})`, value: { lineup: currentLineup } });
        // Also offer moving to the other lineup top level
        options.push({ label: `Top level (${otherLabel})`, value: { lineup: otherLineup } });
    } else {
        // At top level: offer the other lineup
        options.push({ label: `${otherLabel} lineup`, value: { lineup: otherLineup } });
        // Offer subfolders in current lineup
        library.games.forEach(g => {
            if (g.is_folder) {
                const name = g.game.display.name_eng || g.folder;
                options.push({ label: `📁 ${name}`, value: { lineup: currentLineup, folder: g.folder } });
            }
        });
        // Offer subfolders in other lineup
        const otherLib = dualLibrary[otherLineup];
        if (otherLib) {
            otherLib.games.forEach(g => {
                if (g.is_folder) {
                    const name = g.game.display.name_eng || g.folder;
                    options.push({ label: `📁 ${name} (${otherLabel})`, value: { lineup: otherLineup, folder: g.folder } });
                }
            });
        }
    }

    if (options.length === 0) return;

    const dest = await modalSelect('Move "' + (entry.game.display.name_eng || entry.game.display.name) + '" to...', options);
    if (!dest) return;

    // Build src and dest lineup paths for the Rust command
    const srcLineup = currentFolder
        ? currentLineup + '/' + currentFolder.name
        : currentLineup;
    const destLineup = dest.folder
        ? dest.lineup + '/' + dest.folder
        : dest.lineup;

    try {
        // Move on disk, get final folder name (may be renamed on conflict)
        const finalName = await invoke('move_game', {
            gamesPath: dualLibrary.path,
            srcLineup: srcLineup,
            folderName: entry.folder,
            destLineup: destLineup
        });

        // Remove from source list
        const movedIndex = selectedIndex;
        library.games.splice(movedIndex, 1);
        recomputeSortIndices(library.games);

        // Re-read the moved game entry from disk (country may have changed)
        const movedEntry = await invoke('load_game_entry', {
            gamesPath: dualLibrary.path,
            lineup: destLineup,
            folderName: finalName
        });

        // Add to destination list
        // Note: when inside a subfolder, dualLibrary[lineup].games points to the
        // subfolder's games (swapped by enterFolder), so we must use
        // currentFolder.parentGames to access the top-level list.
        if (dest.folder) {
            // Moving into a subfolder — load its contents, add, save
            const folderLib = await invoke('load_folder_contents', {
                gamesPath: dualLibrary.path,
                lineup: dest.lineup,
                folderName: dest.folder
            });
            movedEntry.sort.sor_demo = folderLib.games.length;
            movedEntry.sort.sor_pnum = folderLib.games.length;
            movedEntry.sort.sor_date = folderLib.games.length;
            movedEntry.sort.sor_name = folderLib.games.length;
            movedEntry.sort.sor_genr = folderLib.games.length;
            folderLib.games.push(movedEntry);
            recomputeSortIndices(folderLib.games);
            await invoke('save_folder_contents', {
                gamesPath: dualLibrary.path,
                lineup: dest.lineup,
                folderName: dest.folder,
                library: folderLib
            });
            // Update folder's game_count in parent list
            const destParent = (currentFolder && dest.lineup === currentLineup)
                ? currentFolder.parentGames
                : dualLibrary[dest.lineup].games;
            const folderEntry = destParent.find(g => g.folder === dest.folder);
            if (folderEntry) folderEntry.game_count = folderLib.games.length;
        } else {
            // Moving to top level of a lineup
            const targetGames = (currentFolder && dest.lineup === currentLineup)
                ? currentFolder.parentGames
                : dualLibrary[dest.lineup].games;
            movedEntry.sort.sor_demo = targetGames.length;
            movedEntry.sort.sor_pnum = targetGames.length;
            movedEntry.sort.sor_date = targetGames.length;
            movedEntry.sort.sor_name = targetGames.length;
            movedEntry.sort.sor_genr = targetGames.length;
            targetGames.push(movedEntry);
            recomputeSortIndices(targetGames);

            // Apply forced titlebar if moving to US lineup
            if (dest.lineup === 'us' && editorSettings.force_us_titlebar) {
                const e = targetGames[targetGames.length - 1];
                e.game.display.titlebar = (e.game.display.csize <= 1) ? 9 : 10;
            }
        }

        // Save source subfolder contents and update parent game_count
        if (currentFolder) {
            await invoke('save_folder_contents', {
                gamesPath: dualLibrary.path,
                lineup: currentLineup,
                folderName: currentFolder.name,
                library: library
            });
            const parentEntry = currentFolder.parentGames.find(g => g.folder === currentFolder.name);
            if (parentEntry) parentEntry.game_count = library.games.length;
        }

        // Save top-level lineups: temporarily restore parent games if inside a subfolder,
        // since dualLibrary[lineup].games currently points to subfolder contents
        let subfolderGames = null;
        if (currentFolder) {
            subfolderGames = dualLibrary[currentLineup].games;
            dualLibrary[currentLineup].games = currentFolder.parentGames;
        }
        await invoke('save_library', { gamesPath: dualLibrary.path, library: dualLibrary });
        if (subfolderGames) {
            dualLibrary[currentLineup].games = subfolderGames;
        }

        // Update UI: auto-select next game in current list
        selectedIndex = -1;
        renderGameList();
        updateStats();
        if (library.games.length > 0) {
            const nextIndex = movedIndex < library.games.length ? movedIndex : library.games.length - 1;
            const nextEntry = library.games[nextIndex];
            if (nextEntry.is_folder) selectFolder(nextIndex);
            else selectGame(nextIndex);
        } else {
            document.getElementById('editor').classList.remove('active');
            document.getElementById('folder-editor').style.display = 'none';
            document.getElementById('placeholder').style.display = '';
        }
    } catch (e) {
        modalAlert('Error moving game: ' + e);
    }
}

// ---- Delete game ----
async function deleteGame() {
    const library = getLibrary();
    if (selectedIndex < 0) return;
    const entry = library.games[selectedIndex];
    const isFolder = entry.is_folder;
    const what = isFolder ? 'folder' : 'game';
    const warning = isFolder
        ? `Delete folder "${entry.game.display.name_eng}" and ALL games inside?\n\nThis cannot be undone.`
        : `Delete "${entry.game.display.name_eng || entry.game.display.name}" and its folder?\n\nThis cannot be undone.`;
    if (editorSettings.confirm_delete && !await modalConfirm(warning)) return;

    const lineupForDelete = currentFolder
        ? currentLineup + '/' + currentFolder.name
        : currentLineup;

    try {
        const cmd = isFolder ? 'delete_folder' : 'delete_game';
        await invoke(cmd, {
            gamesPath: dualLibrary.path,
            lineup: lineupForDelete,
            folderName: entry.folder
        });
        const deletedIndex = selectedIndex;
        library.games.splice(deletedIndex, 1);

        // Recompute all sort indices (gaps cause phantom entries in emu)
        recomputeSortIndices(library.games);
        // Save gamelist.json immediately so it stays in sync with disk
        if (currentFolder) {
            await invoke('save_folder_contents', {
                gamesPath: dualLibrary.path,
                lineup: currentLineup,
                folderName: currentFolder.name,
                library: library
            });
        } else {
            await invoke('save_library', { gamesPath: dualLibrary.path, library: dualLibrary });
        }
        selectedIndex = -1;
        renderGameList();
        updateStats();
        // Auto-select next game, or previous if last was deleted
        if (library.games.length > 0) {
            const nextIndex = deletedIndex < library.games.length ? deletedIndex : library.games.length - 1;
            const nextEntry = library.games[nextIndex];
            if (nextEntry.is_folder) {
                selectFolder(nextIndex);
            } else {
                selectGame(nextIndex);
            }
        } else {
            document.getElementById('editor').classList.remove('active');
            document.getElementById('folder-editor').style.display = 'none';
            document.getElementById('placeholder').style.display = '';
        }
    } catch (e) {
        modalAlert('Error deleting game: ' + e);
    }
}

// ---- Auto-save ----
// Debounced save: writes to disk after 300ms of inactivity.
function autoSave() {
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => saveNow(), 300);
}

async function saveNow() {
    if (!dualLibrary) return;
    try {
        if (currentFolder) {
            await invoke('save_folder_contents', {
                gamesPath: dualLibrary.path,
                lineup: currentLineup,
                folderName: currentFolder.name,
                library: getLibrary()
            });
        } else {
            await invoke('save_library', { gamesPath: dualLibrary.path, library: dualLibrary });
        }
    } catch (e) {
        console.error('Auto-save failed:', e);
    }
}

// ---- Helpers ----
function escHtml(s) {
    const div = document.createElement('div');
    div.textContent = s;
    return div.innerHTML;
}

// ---- Publish ----

// Wraps the m2-publish Rust crate via the publish_library Tauri command.
// Asks the user where to write the publish output and which stock alldata
// tree to use as PSB templates.
async function publishLibrary() {
    if (!dualLibrary || !dualLibrary.path) {
        modalAlert('Open a library folder first');
        return;
    }
    // Wrapper layout: backend auto-resolves stockRoot to <wrapper>/templates/
    // and outRoot to <wrapper>/published/ when we pass empty strings.
    // dualLibrary.path is the resolved library root; backend's
    // resolve_library_paths handles both wrapper and legacy layouts.
    try {
        const result = await invoke('publish_library', {
            gamesPath: dualLibrary.path,
            stockDataRoot: '',
            outputRoot: '',
        });
        modalAlert(
            `Publish OK\n\n` +
            `ROMs packed (mzs):  ${result.roms_packed}\n` +
            `ROMs copied:        ${result.roms_copied}\n` +
            `PSB files written:  ${result.psb_files_written}\n` +
            `Folders emitted:    ${result.folders_emitted}\n` +
            `Save files copied:  ${result.save_files_copied}\n` +
            `SRAM blocks embedded: ${result.sram_blocks_embedded}\n\n` +
            `Output: ${result.output_root}`
        );
    } catch (e) {
        modalAlert('Publish failed: ' + e);
    }
}

