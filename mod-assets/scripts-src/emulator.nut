//----------------------------------------------------------------------
// EmuTask関係
//----------------------------------------------------------------------

// EmuTaskはsqobject::Objectを継承している

class EmuTaskWrapper extends EmuTask {
  emusubst_rsc = null;
  emusubst_resourceId = null;
  emusubst_layer = null;
  // emusubst_layer_scale = null;
  emusubst_motion = null;

  sheet_layer = null;
  // sheet_layer_scale = null;
  sheet_motion = null;

  sheet_size_margin2X = null;

  b_wide_mode = null;
  b_emu_DbD_mode = null;

  aoe_opacity     = null;
  aoe_opacity_end = null;
  aoe_delta       = null;
  aoe_timer       = null;
  aoe_running     = null;
  aoe_thread = null;

  static sheet_opacity = 192;
  static sheet_size_margin2X_DEF = (- DIALOG_CENTER_SIZE);
  static sheet_size_margin2X_DbD = (0);

  static notify_aoe_end   = "etw_aoe_end";
  static notify_aoe_start = "etw_aoe_start";

  ram_check       = null;

  sramHotswap         = false;
  initializeSuccess   = false;
  state_check_control = null;
  state_check_result  = false;
  standalone_control  = null;
  struct_rsc = null;
  struct_statedata_extend = null;

  m_attr = null;

  b_change_width = null;
  old_width = null;

  // ----------------------------------
  // コンストラクタ
  // ----------------------------------
  constructor() {
    ::EmuTask.constructor( this._get_layerFolder() );

    create_sheet(); // 下敷きを生成 

    this._init_struct();

    // 透明度アニメ用thread起動
    aoe_opacity     = 0;
    aoe_opacity_end = 0;
    aoe_delta       = 0;
    aoe_timer       = 0;
    aoe_running     = false;
    aoe_thread = fork( _anime_opacity_thread_func.bindenv(this) );

    ram_check       = 0;

    sramHotswap         = false;
    initializeSuccess   = false;
    state_check_control = null;
    state_check_result  = false;
    standalone_control  = null;

    m_attr = {};

    b_change_width = false;
  }

  // ----------------------------------
  // デストラクタ
  // ----------------------------------
  function destructor() {
  }

  function _get_layerFolder() {
    return ::util_get_game_screen_layerFolder();
  }

  // ----------------------------------
  // 下敷きを生成
  // EmuTaskのコンストラクタを前もって実行しておくこと
  // ----------------------------------
  function create_sheet() {
    // レイヤを作成
    sheet_layer = Layer( this._get_layerFolder() );
    sheet_layer.priority = ::EmuTask.getPriority() - 2; // emu画面より奥に
    sheet_layer.smoothing = true;

    sheet_layer.visible = false;

    ::register_dialog_motion_to_layer(sheet_layer); // レイヤに標準ダイアログモーションリソースを登録

    sheet_motion = Motion(sheet_layer);
    sheet_motion.visible = false;
//    sheet_motion.chara = "dialog";	//PCEではエミュレーターウィンドウの影を非表示にします。
//    sheet_motion.motion = "stand";
    sheet_motion.independentLayerInherit = true;
    sheet_motion.opacity = sheet_opacity;
    this._set_sheet_visible();

    sheet_size_margin2X = sheet_size_margin2X_DEF;

    // 
    b_wide_mode = ::is_screen_mode_wide();
    b_emu_DbD_mode = false;
  }

  // ----------------------------------
  // スクリーン設定用の代用emu画面を生成
  // ----------------------------------
  function create_emusubst() {
    local title_emu_screen_state_path = _get_title_emu_screen_state_path();
    if (title_emu_screen_state_path) {
      setup_emulator_statedata(title_emu_screen_state_path);
    }
    else {
      // 必要なリソースをロード
      local emusubst_motion_path = conv_path(MENU_EMU_SCREEN_MOTION_PATH);
      emusubst_rsc = Resource();
      emusubst_rsc.load(emusubst_motion_path);
      while (emusubst_rsc.loading)
        wait(0);

      // レイヤを作成
      emusubst_layer = Layer( this._get_layerFolder() );
      emusubst_layer.priority  = ::EmuTask.getPriority() - 1; // emu画面より奥に
      emusubst_layer.smoothing = ::EmuTask.getSmoothing();
      emusubst_layer.visible   = ::EmuTask.getVisible();

      // レイヤにモーションリソースを登録
      emusubst_resourceId = emusubst_layer.registerMotionResource(emusubst_rsc.find(emusubst_motion_path));

      emusubst_motion = Motion(emusubst_layer);
      emusubst_motion.visible = ::EmuTask.getVisible();
      emusubst_motion.chara = "emu_screen";
      emusubst_motion.motion = "stand";
      emusubst_motion.independentLayerInherit = true;
      emusubst_motion.opacity = ::EmuTask.getOpacity();

      emusubst_motion.zoomX = this._get_emusubst_motion_zoomX();
      emusubst_motion.zoomY = ::EmuTask.getScaleY();
      emusubst_motion.left  = ::EmuTask.getPosX();
      emusubst_motion.top   = ::EmuTask.getPosY();

      ::EmuTask.setEmuDraw(false);
    }
  }

  function _get_emusubst_motion_zoomX() {
    local sx = ::EmuTask.getScaleX();
    local title_prof = get_current_title_prof();
    if ("subst_screen_zoom_x" in  title_prof.m2epi) {
      sx *= title_prof.m2epi.subst_screen_zoom_x;
    }
    return sx;
  }
  
  // ----------------------------------
  // スクリーン設定用の代用emu画面を解放
  // ----------------------------------
  function delete_emusubst() {
    // レイヤからモーションリソースを登録解除
    if (emusubst_resourceId) 
      emusubst_layer.unregisterMotionResource(emusubst_resourceId);
    emusubst_resourceId = null;

    if (emusubst_motion) {
      emusubst_motion = null;
      emusubst_layer = null;
      emusubst_rsc = null;

      ::EmuTask.setEmuDraw(true);
    }
    else {
      request_reset_emulator();
    }
  }

  function _init_struct() {
    local struct_statedata_extend_path = conv_path(STRUCT_STATEDATA_EXTEND_PATH);
    struct_rsc = Resource();
    struct_rsc.load(struct_statedata_extend_path);
    while (struct_rsc.loading)
      wait(0);

    struct_statedata_extend = Struct(struct_rsc.find(struct_statedata_extend_path));
  }

  // ----------------------------------
  // 
  // ----------------------------------
  function set_emu_DbD_mode(_b) {
    b_emu_DbD_mode = _b;
  }

  // ----------------------------------
  // 
  // ----------------------------------
  function calc_sheet_size_margin2X() {
    // ScreenBoundsが4:3
    // emu画面がDotByDotモード
    if ((b_wide_mode == false) 
        && (b_emu_DbD_mode == true)
        ) {
      sheet_size_margin2X = sheet_size_margin2X_DbD;
    }
    else {
      sheet_size_margin2X = sheet_size_margin2X_DEF;
    }
  }

  // ----------------------------------
  // （下敷きのみ）表示/非表示
  // ----------------------------------
  function setSheetVisible(_b) {
    //        printf("EmuTaskWrapper::setSheetVisible( %s )\n", _b.tostring());
    //        sheet_motion.visible = _b;
    sheet_layer.visible = _b;
  }

  // ----------------------------------
  // 表示設定系のオーバーライド
  // ----------------------------------

  function setPriority(_val) {
    //        printf("EmuTaskWrapper::setPriority( %d )\n", _val);
    ::EmuTask.setPriority( _val );
    sheet_layer.priority = _val - 2; // emu画面より奥に
    if (emusubst_layer) 
      emusubst_layer.priority = _val - 1; // emu画面より奥に
  }

  function setSmoothing(_b) {
    // printf("EmuTaskWrapper::setSmoothing( %s )\n", _b.tostring());
    ::EmuTask.setSmoothing( _b );
    if (emusubst_layer) 
      emusubst_layer.smoothing = _b;
  }

  function setVisible(_b) {
    //        printf("EmuTaskWrapper::setVisible( %s )\n", _b.tostring());
    ::EmuTask.setVisible( _b );
    sheet_motion.visible = _b;
    sheet_layer.visible = _b;
    if (emusubst_layer) {
      emusubst_motion.visible = _b;
      emusubst_layer.visible = _b;
    }
  }

  function _setOpacity_Core(_val) {
    ::EmuTask.setOpacity( _val );
    sheet_motion.opacity = (_val > sheet_opacity) ? sheet_opacity : _val;
    this._set_sheet_visible();
    
    if (emusubst_motion) 
      emusubst_motion.opacity = _val;
  }

  function _set_sheet_visible() {
    local center_motion = sheet_motion.getLayerSetter("レイヤ1");
    if (center_motion) {
      center_motion.visible = (::EmuTask.getOpacity() == 255) ? false : true;  // 中央パーツを非表示に
    }
  }

  function setOpacity(_val) {
    if (aoe_running) {
      aoe_delta = 0;
    }
    _setOpacity_Core(_val);
  }

  function setCoord(_x, _y) {
    this.setPosX(_x);
    this.setPosY(_y);
  }

  function setPosX(_x) {
    ::EmuTask.setPosX( _x );
    sheet_motion.left = _x;
    sheet_motion.left = _x;
    if (emusubst_motion) 
      emusubst_motion.left = _x;
  }

  function setPosY(_y) {
    ::EmuTask.setPosY( _y );
    sheet_motion.top = _y;
    if (emusubst_motion) 
      emusubst_motion.top = _y;
  }

  function setRotZdegree(_deg) {
    ::EmuTask.setRotZdegree(_deg);
    sheet_motion.angleDeg = _deg;
    if (emusubst_motion) 
      emusubst_motion.angleDeg = _deg;
  }

  function setScale(_x, _y) {
    this.setScaleX(_x);
    this.setScaleY(_y);
  }

  function setScaleX(_x) {
    ::EmuTask.setScaleX( _x );
    this.calc_sheet_size_margin2X();
    local _size_x = (::EmuTask.getWidth() * abs(_x)) +sheet_size_margin2X;
    if (_size_x < 0.0)
      _size_x = 0.0;
    sheet_motion.zoomX = _size_x / DIALOG_CENTER_SIZE;
    if (emusubst_motion) 
      emusubst_motion.zoomX = this._get_emusubst_motion_zoomX();
  }

  function setScaleY(_y) {
    ::EmuTask.setScaleY( _y );
    this.calc_sheet_size_margin2X();
    local _size_y = (::EmuTask.getHeight() * abs(_y)) +sheet_size_margin2X;
    if (_size_y < 0.0)
      _size_y = 0.0;
    sheet_motion.zoomY = _size_y / DIALOG_CENTER_SIZE;
    if (emusubst_motion) 
      emusubst_motion.zoomY = _y;
  }

  function setScreen(_pos_x, _pos_y, _size_x, _size_y) {
    this.setCoord( _pos_x, _pos_y );
    this.setScale( _size_x / 100.0, _size_y / 100.0 );
  }

  // ----------------------------------
  // 透明度アニメ
  // ----------------------------------
  function _anime_opacity_thread_func() {
    aoe_running = true;
    //        print("start _anime_opacity_thread_func()\n");
    while (1) {
      if (aoe_delta < 0 || aoe_delta > 0) {
        local opa = aoe_opacity + aoe_delta;
        aoe_timer --;
        if (0 >= aoe_timer) {
          opa = aoe_opacity_end;    // 終了時は終了値を明示的にセット
          aoe_delta = 0;
        }
        if (opa <   0.0)
          opa =   0.0;
        if (opa > 255.0)
          opa = 255.0;
        aoe_opacity = opa;

        _setOpacity_Core( opa.tointeger() );

        wait(0);
      }
      else  {
        ::notify( notify_aoe_end );
        //                printf("sleep _anime_opacity_thread_func() : ::EmuTask.opacity is %d\n", ::EmuTask.getOpacity());
        aoe_running = false;
        wait( notify_aoe_start );
        //                print("wakeup _anime_opacity_thread_func()\n");
        aoe_running = true;
      }
    }
        
    aoe_running = false;
  }

  function animateOpacity(_opacity, frameCount) {
    //        printf("EmuTaskWrapper::animateOpacity( %d, %d )\n", _opacity, frameCount);
    aoe_opacity     = ::EmuTask.getOpacity().tofloat();
    aoe_opacity_end = _opacity;
    aoe_delta = (_opacity - ::EmuTask.getOpacity()).tofloat() / frameCount.tofloat();
    aoe_timer = frameCount;
    //        printf("aoe_delta : %f\n", aoe_delta);

    ::notify( notify_aoe_start );
    //        wait( notify_aoe_end );
    wait(0);
  }

  function getAnimating() {
    local running = 0;

    if (aoe_running == true) 
      running = running | 1;

    return (running == 1);
  }

  // callback
  function onFilepath(params) {
    local checked_path = conv_checked_path(params.short_path, params.dev_id);
    if (checked_path == null) 
      checked_path = "";    // 空文字にしておく
    printf("onFilepath(\"%s\", %d) : %s\n", params.short_path, params.dev_id, checked_path);
        
    params.short_path = checked_path;
    params.dev_id = get_conv_path_result_dev_id();

    return params;
  }

  // callback
  function onGetBackupObjects(params) {
    local ret = null;
    return ret;
  }

  // callback
  // (EmuTaskの初期化プロセス完了時にコールされる)
  function onInitializeFinish() {
    printf("onInitializeFinish() : \n");
    // 
    setMemoryPatch(1);                   // メモリパッチを即時実行
    s_init_emulator_rom_change = false;  // ROM切り替えの必要性をクリア
  }

  // callback
  // (machine_restart()完了直後にコールされる)
  function onMachineRestart() {
    printf("onMachineRestart() : \n");

    // メモリパッチを即時実行
    setMemoryPatch(1);
  }

  // callback
  // (NowLoading表示ON/OFFのシグナルをsqに伝えるためのコールバック)
  // (コールバック内で待ち処理が発生しないよう気を付けること)
  function onNowLoading(b, _type) {
    printf("onNowLoading() : %s, %d\n", b.tostring(), _type);

    // XXX : とりあえず何も考えないで制御しとく
    if (b) {
      ::g_menu_nowloading.setCoord(0,0);
//      ::g_menu_nowloading.open(NowLoadingTypes.NOW_LOADING + _type, null);
    }
    else {
//      ::g_menu_nowloading.close();
    }

  }

  // callback
  // emutaskが1frame分処理する度に呼ばれる
  function onMachineRun() {
    if (state_check_control) {
      if (state_check_control.update_status(this))
        state_check_result = true;
    }
  }

  // callback
  // 解像度変更を検知したときに呼ばれる
  function onChangeResolution() {
    printf("onChangeResolution() : \n");
    if (standalone_control) {
      standalone_control.on_resolution_change(this, ::g_systemdata);
    }
    else {
      if (::g_systemdata) {
        ::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).update_system();
      }
    }
  }

  // OnTaskに同期したState Check処理登録
  function setStateCheckControl(_state_check_control) {
    state_check_result  = false;
    state_check_control = _state_check_control;
  }
  function getStateCheckControl() {
    return state_check_control;
  }

  function getStateCheckResult() {
    if (state_check_control) {
      return state_check_result;
    }
    else {
      return true;
    }
  }

  function setStandaloneControl(_stanealone_control) {
    standalone_control = _stanealone_control;
  }

  function getStandaloneControl() {
    return standalone_control;
  }

  // ApplyStateでRestartする直前に呼ばれる
  function onBeforeApplyStateRestart() {
    if (standalone_control)
      standalone_control.before_apply_state(this); // タイトル個別のstateロード時に退避する情報があれば実行
  }

  // ApplyStateでRestartする直後に呼ばれる
  function onAfterApplyStateRestart() {
    if (standalone_control)
      standalone_control.after_apply_state(this); // タイトル個別のstateロード時に退避した情報があれば書き戻し
  }

  // callback
  function onCaptureHistory() {
    // State Checkの状態を保存
    printf("\tonCaptureHistory\n");
    if (state_check_control) {
      state_check_control.save_status( struct_statedata_extend );
    }
    this.SetStateDataExtendFromStruct( struct_statedata_extend );
  }

  // callback
  function onApplyHistory() {
    // State Checkの状態を復帰
    printf("\tonApplyHistory\n");
    this.GetStateDataExtendToStruct( struct_statedata_extend );
    if (state_check_control) {
      state_check_control.load_status( struct_statedata_extend );
    }
  }

  // callback
  function onChangeGeometry() {
    printf("\tonChangeGeometry: now width=%s, height=%s\n", ::EmuTask.getWidth(), ::EmuTask.getHeight());
    this.setScale( ::EmuTask.getScaleX(), ::EmuTask.getScaleY() );
    if (::g_systemdata) {
      local noUpdateWallpaper = true;
      ::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).update_system(noUpdateWallpaper);	// 反映
    }
    // 解像度が変更されたかをチェック
    if (old_width == null) old_width = ::EmuTask.getWidth();  // 初回はnullなのでチェック前に設定
    b_change_width = false;
    if (old_width != ::EmuTask.getWidth()) {
      b_change_width = true;
    }
    old_width = ::EmuTask.getWidth();
    
    //画面設定の反映を入れてみる
		setDisplayScale( ::EmuTask.getWidth(), ::EmuTask.getHeight() );
    
  }

  function setRamCheck()
  {
    printf(">>>setRamCheck\n");

    ram_check = 1;
  }

  // エミュレータメモリに対する書き換えを行う
  // 　エミュレータブート時にBIOSでRAMが0クリアされるため
  // 　エミュレータリセット後、machine_runが１回呼ばれた後にも必ず呼び出すこと
  function setMemoryPatch(flg=0)
  {
    if ( ram_check || flg ) {
      printf(">>>setMemoryPatch\n");

      // original_bug
      setOriginalBugData();
      // default patch
      setDefaultPatch();

      if ( flg ) {
        // ここではクリアせずブート後にも書き換えを行わせる
        ram_check = 1;
      }
      else {
        ram_check = 0;
      }
    }
  }

  function setAttribute( _attr_name, _value ) {
    ::EmuTask.setAttribute( _attr_name, _value);
    if (_attr_name in m_attr) {
      m_attr[_attr_name] = _value;
    }
    else {
      m_attr[_attr_name] <- _value;
    }
  }

  function getAttribute( _attr_name ) {
    return m_attr[_attr_name];
  }

};

function EmuTaskWrapper::getInitializeSuccess()
{
  return initializeSuccess;
}

// -------------------------------------------
// emulator 初期化（&実行）
// -------------------------------------------

s_rom_fileparse <- null;                   // path分解されたromのpath
s_init_emulator_langTag_current <- null;   // 最後に初期化したときのlangTag（現在のlangTag）
s_init_emulator_langTag_request <- null;   // 再初期化リクエストlangTag

s_init_emulator_regionTag_current <- null; // 最後に初期化したときのregionTag（現在のregionTag）
s_init_emulator_regionTag_request <- null; // 再初期化リクエストregionTag

s_init_emulator_rom_current <- null;       // 最後に初期化したときのrom（現在のrom）
s_init_emulator_arch_current <- null;      // 最後に初期化したときのromのハードウエア
s_init_emulator_rom_request <- null;       // 再初期化リクエストrom
s_init_emulator_arch_request <- null;      // 再初期化リクエストromのハードウエア
s_init_emulator_preamp_request <- null;    // 再初期化リクエストpreamp
s_init_emulator_rom_change <- null         // 初期化時romファイル読み込みの必要性

s_init_emulator_overdrive <- null;         // 最後に初期化したときのoverdrive値


// emulator 初期化
function init_emulator(regionTag=null)
{
  local ret = false;

  if (regionTag == null) {
    regionTag = ::g_systemdata.get_data_setting_etc__game_regionTag();
  }

  regionTag = ::chronos_resolve_game_region(regionTag);

  // Activate the stable owner before native initialization/applying SRAM.
  ::chronos_activate_game_saves(regionTag);
  try {


  // EmuTask 生成
  if (null == ::g_emu_task) {
    ::g_emu_task = EmuTaskWrapper();
    ::g_emu_task.setInputDirectionMerge(DirectionMergeFlag.LEFT_STICK);
  }

  // EmuTask 初期化開始
  if (::g_emu_task) {
    local title_prof = ::get_current_title_prof();
    local dev_name   = ::get_title_dev_name( ::get_current_title_dev_id() );
    local input_num  = ::util_get_player_input_num(title_prof, regionTag);
    local m2epi      = title_prof.m2epi;

    // optional
    if ( ::util_is_sram_support(regionTag) && ("sram_hot_swap" in m2epi) ) {
      ::g_emu_task.sramHotswap = m2epi["sram_hot_swap"];
    }
    if ( ::util_is_sram_support(regionTag) && ("sram_crypted" in m2epi) ) {
      ::g_emu_task.sramCrypted = m2epi["sram_crypted"];
    }

    local country       = m2epi.version[regionTag].country;
    local arch          = _get_emu_arch(regionTag);
    local rom           = _get_rom_path(regionTag);
    local preamp        = ::_get_emulator_preamp(regionTag);                      // optional
    local overdrive     = ::_init_emulator_get_option_by_regionTag(m2epi, regionTag, "overdrive",  0);   // optional
    local brightness    = ::_init_emulator_get_option_by_regionTag(m2epi, regionTag, "brightness", 1.0); // optional
    local archSubInfo   = ::_init_emulator_get_arch_sub_info(m2epi, regionTag);   // optional
    local archExtraInfo = ::_init_emulator_get_arch_extra_info(m2epi, regionTag); // optional
    local attrInfo      = ::_init_emulator_get_attr_option(m2epi, regionTag);     // optional
    local stringInfo    = ::_init_emulator_get_string_option(m2epi, regionTag);   // optional
    local splitInfo     = ::_init_emulator_get_view_split_num(m2epi, regionTag);  // optional
    local stringGlobal  = ::_init_emulator_get_option_by_regionTag(m2epi, regionTag, "stringGlobal"); // optional

    // s_init_emulator_rom_requestで指定したROMを読み込むように修正 by saito
    if (s_init_emulator_rom_request != null) {
      rom = conv_path( s_init_emulator_rom_request );
      if (s_init_emulator_arch_request != null) {
        arch = s_init_emulator_arch_request;
      }
      if (s_init_emulator_preamp_request != null) {
        preamp = s_init_emulator_preamp_request;
        s_init_emulator_preamp_request = null;
      }
      s_init_emulator_rom_request = null;
    }

    if ( (s_init_emulator_rom_current != rom)||(s_init_emulator_arch_current != arch) ) {
      s_init_emulator_rom_current = rom; // 最後に初期化したときのrom（現在のrom）
      s_init_emulator_arch_current = arch;
      s_init_emulator_rom_change = true; // rom切り替えの必要性を設定
    }

    // romのpathからファイル名のみ抽出して保存しておく(デバッグ用)
    {
      printf("regionTag : %s\n", regionTag);
      printf("country   : %s\n", country);
      printf("arch      : %s\n", arch);
      printf("preamp    : %s\n", preamp);
      s_rom_fileparse = fileparse( rom );
      printf("parsed rom's path (%s): \n", rom);
      printf("\t path : \"%s\"\n", s_rom_fileparse.path);
      printf("\t name : \"%s\"\n", s_rom_fileparse.name);
      printf("\t ext  : \"%s\"\n", s_rom_fileparse.ext);
    }

  //  _init_emulator_set_table_value(m2epi, regionTag, "stringGlobal", "setStringGlobal");

    ::g_emu_task.priority = PRIORITY_EMULATOR_LAYER;
    ::g_emu_task.brightness = brightness;
    ::g_emu_task.setLogFilename( format("%s_%s.log", dev_name, regionTag) );
    ::g_emu_task.initializeSuccess = ::g_emu_task.init(input_num, 
                                                       country,
                                                       arch, 
                                                       rom,
                                                       archSubInfo, 
                                                       archExtraInfo,
                                                       preamp,
                                                       s_init_emulator_rom_change
                                                       );
    ret = ::g_emu_task.initializeSuccess;

    s_init_emulator_overdrive = 
      ::g_emu_task.overdrive = overdrive;

    // emulatorリセットカウント初期化
    init_reset_emulator_count();

    if ( null != attrInfo ) {
      while ( !::g_emu_task.initialized ) {
        wait(0); // XXX 初期化が終わらないとsetAttributeできないため、待つ(本来attrはゲーム設定内で定義すべき)
      }
      foreach ( key,val in attrInfo ) {
        ::g_emu_task.setAttribute( key, val );
      }
    }
    if ( null != stringInfo ) {
      while ( !::g_emu_task.initialized ) {
        wait(0); // XXX 初期化待ち
      }
      _init_emulator_set_table_value(m2epi, regionTag, "string", "setString");
    }
    if ( null != splitInfo ) {
      ::g_emu_task.setViewSplitNumX(splitInfo[0]);
      ::g_emu_task.setViewSplitNumY(splitInfo[1]);
    }
    else {
      ::g_emu_task.setViewSplitNumX(1);
      ::g_emu_task.setViewSplitNumY(1);
    }
    if ( ::util_is_sram_support(regionTag) && ("sram_need_save_frame" in m2epi) ) {
      while ( !::g_emu_task.initialized ) {
        wait(0); // XXX 初期化が終わらないとmachineに届かないため、待つ
      }
      ::g_emu_task.setSramNeedSaveFrame( m2epi["sram_need_save_frame"] );
    }
    // 初期化完了後、emulatorはタスクでpollingされ続ける
    // 初期化完了直後はpause状態
  }

  // 最後に初期化したときのregionTag（現在のregionTag）
  s_init_emulator_regionTag_current = regionTag.slice(0);

  if (::chronos_save_enabled() && ret &&
      (::s_chronos_active_game_id != null || ::s_chronos_pending_game_id != null)) {
    while (!::g_emu_task.initialized) wait(0);
    local sram = ::g_systemdata.get_value(SystemDataValueIndex.SRAM_DATA);
    // Platform profiles can disable the stock utility adapter. V2 owns
    // common slot0 explicitly and applies it through the native SRAM API.
    local applied = sram.m_emu_sram[0].apply_to_emu(::g_emu_task);
    if (!applied && sram.get_current_data()[0]["_00_sram_entries"][0]["_03_size_dec"] > 0)
      throw "Cannot apply the selected game SRAM; its saved file was preserved";
  }

  // (保険)
  // emulator 再初期化 リクエスト登録
  ::chronos_complete_game_init(ret);

  request_re_init_emulator(s_init_emulator_regionTag_current,  s_init_emulator_rom_current);

  return ret;
  } catch (e) {
    if (::chronos_save_enabled()) ::chronos_abort_game_init();
    throw e;
  }
}

function _init_emulator_set_table_value(_m2epi, _regionTag, _key, _func)
{
  local value_common  = null;
  if ( _key in _m2epi ) {
    value_common = _m2epi[_key];
  }
  local value_version = null;
  if ( _key in _m2epi["version"][_regionTag] ) {
    value_version = _m2epi["version"][_regionTag][_key];
  }

  if ( null != value_common ) {
    foreach ( key,val in value_common ) {
      if ( null != value_version && key in value_version ) {
        continue; // version個別側に定義があるものはスキップ
      }
      ::g_emu_task[_func]( key, val );
    }
  }
  
  if ( null != value_version ) {
    foreach ( key,val in value_version ) {
      ::g_emu_task[_func]( key, val );
    }
  }
}

function _init_emulator_get_option_by_regionTag(_m2epi, _regionTag, _key, _default_value = null)
{
  local value = _default_value;
  if ( _key in _m2epi ) {
    value = _m2epi[_key];
  }
  if ( _key in _m2epi["version"][_regionTag] ) {
    value = _m2epi["version"][_regionTag][_key];
  }
  return value;
}

function _init_emulator_get_arch_sub_info(_m2epi, _regionTag)
{
  local archSubInfo = 0;
  local disable_md_6B = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "disable_md_6B");
  if ( null != disable_md_6B ) {
    archSubInfo = archSubInfo | (disable_md_6B ? M2EpiArchSubInfo.MD_6B_DISABLE : 0);
  }
//  local md_multi_tap = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "md_multitap") ;
//  if ( null != md_multi_tap ) {
//    archSubInfo = archSubInfo | (md_multi_tap & 0x01 ? M2EpiArchSubInfo.MD_MULTITAP_0 : 0);
//    archSubInfo = archSubInfo | (md_multi_tap & 0x02 ? M2EpiArchSubInfo.MD_MULTITAP_1 : 0);
//  }
  local use_analog = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "use_analog");
  if ( null != use_analog ) {
    archSubInfo = archSubInfo | (use_analog ? M2EpiArchSubInfo.USE_ANALOG    : 0);
  }
  local direction = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "direction");
  if ( null != direction ) {
    archSubInfo = archSubInfo | (direction.tostring() == "4" ? M2EpiArchSubInfo.DIRECTION_4 : 0);
  }
//  local use_subthread = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "use_subthread");
//  if ( null != use_subthread ) {
//    archSubInfo = archSubInfo | (use_subthread ? M2EpiArchSubInfo.USE_SUBTHREAD : 0);
//  }
  local systemcard = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "tg16cd_systemcard");
  if ( null != systemcard ) {
    archSubInfo = archSubInfo | (systemcard.tostring() == "super"  ? M2EpiArchSubInfo.TG16CD_SUPER_CARD  : 0);
    archSubInfo = archSubInfo | (systemcard.tostring() == "arcade" ? M2EpiArchSubInfo.TG16CD_ARCADE_CARD : 0);
  }
  local tg16pad = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "tg16pad_type");
  if ( null == tg16pad ) {
    tg16pad = 1;
  }
  archSubInfo = archSubInfo | ((tg16pad & 7) << M2EpiArchSubInfo.TG16PAD_TYPE_SHIFT);

  local tg16hblank = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "tg16_hblank");
  if ( null != tg16hblank ) {
    archSubInfo = archSubInfo | ((tg16hblank & 255) << M2EpiArchSubInfo.TG16_HBLANK_SHIFT);
  }

  local tg16option = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "tg16_option");
  if ( null != tg16option ) {
    archSubInfo = archSubInfo | ((tg16option & 15) << M2EpiArchSubInfo.TG16_OPTION_SHIFT);
  }

  return archSubInfo;
}

function _init_emulator_get_arch_extra_info(_m2epi, _regionTag)
{
  local archExtraInfo = 0;
  local value = ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "extra_info");
  if ( null != value ) {
    archExtraInfo = value;
  }

  return archExtraInfo;
}

function _init_emulator_get_attr_option(_m2epi, _regionTag)
{
  return ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "attr");
}

function _init_emulator_get_string_option(_m2epi, _regionTag)
{
  return ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "string");
}

function _init_emulator_get_view_split_num(_m2epi, _regionTag)
{
  return ::_init_emulator_get_option_by_regionTag(_m2epi, _regionTag, "view_split");
}

// 最後に初期化したときのregionTag（現在のregionTag）を返す
function get_last_init_emulator_region()
{
  local regionTag = s_init_emulator_regionTag_current;

  if (regionTag == null) 
    regionTag = ::g_systemdata.get_data_setting_etc__game_regionTag();

  return regionTag;
}

function get_last_init_emulator_rom()
{
  return s_init_emulator_rom_current;
}

function get_init_emulator_rom_change()
{
  return s_init_emulator_rom_change;
}

function get_init_emulator_overdrive()
{
  return s_init_emulator_overdrive;
}
// (debugメニュー用)
function debug_set_init_emulator_overdrive(od)
{
  s_init_emulator_overdrive = od;
}


function _get_title_emu_screen_state_path() {
  local title_emu_screen_state_path = null;
  local title_prof = get_current_title_prof();

  if ( "screen_setting_state" in title_prof.m2epi) {
    local regionTag = ::g_systemdata.get_data_setting_etc__game_regionTag();
    if ( regionTag in title_prof.m2epi["screen_setting_state"] ) {
      title_emu_screen_state_path = conv_path( title_prof.m2epi["screen_setting_state"][regionTag] );
    }
  }
  if ( null == title_emu_screen_state_path) {
    title_emu_screen_state_path = conv_checked_path(MENU_EMU_SCREEN_STATE_PATH);
  }

  return title_emu_screen_state_path;
}

// romファイル名を返す
function _get_rom_path(regionTag)
{
  if (regionTag == null) 
    regionTag = ::g_systemdata.get_data_setting_etc__game_regionTag();

  local title_prof = ::get_current_title_prof();
  local version    = title_prof.m2epi.version[regionTag];
  local rom        = "";
  if ("rom" in version)  {
    rom = conv_path( version.rom );
  }

  return rom;
}

// emuのarchを返す
function _get_emu_arch(regionTag)
{
  if (regionTag == null) 
    regionTag = ::g_systemdata.get_data_setting_etc__game_regionTag();

  local title_prof = ::get_current_title_prof();
  local version    = title_prof.m2epi.version[regionTag];
  local arch       = "";
  if ("arch" in version)  {
    arch = version.arch;
  }

  return arch;
}

// emuのpreampを返す
function get_emulator_preamp()
{
  local regionTag = ::g_systemdata.get_data_setting_etc__game_regionTag();
  return ::_get_emulator_preamp(regionTag);
}

function _get_emulator_preamp(regionTag)
{
  local title_prof = ::get_current_title_prof();
  local version    = title_prof.m2epi.version[regionTag];
  local preamp = 0.0;
  if ("preamp" in title_prof.m2epi) {
    preamp = title_prof.m2epi.preamp;
  }
  if ("preamp" in version)  {
    preamp = version.preamp;
  }

  return preamp;
}


// emulator 再初期化 リクエスト登録
function request_re_init_emulator(regionTag = null, rom = null, arch = null, preamp = null)
{
  if (regionTag == null) 
    regionTag = ::g_systemdata.get_data_setting_etc__game_regionTag();

  s_init_emulator_regionTag_request = regionTag;

  if ( rom == null )
    rom = _get_rom_path(regionTag);

  if ( arch == null )
    arch = _get_emu_arch(regionTag);

  if ( preamp == null )
    preamp = ::_get_emulator_preamp(regionTag);

  s_init_emulator_rom_request    = rom;
  s_init_emulator_arch_request   = arch;
  s_init_emulator_preamp_request = preamp;
}

// 最後にリクエスト登録されたgame_regionTagを返す
function get_last_request_re_init_emulator_regionTag()
{
  local regionTag = s_init_emulator_regionTag_request;

  if (regionTag == null) 
    regionTag = ::g_systemdata.get_data_setting_etc__game_regionTag();

  return regionTag;
}

// 必要ならemulatorを再初期化
function re_init_emulator(noticeKey = null, force_init = false)
{
  local ret = false;

  local regionTag = s_init_emulator_regionTag_request;
  local rom = s_init_emulator_rom_request;
  local arch = s_init_emulator_arch_request ? s_init_emulator_arch_request : s_init_emulator_arch_current;
  if
    (
     (get_last_init_emulator_region() != regionTag) 
     || (get_last_init_emulator_rom() != rom) 
     || (s_init_emulator_arch_current != arch) 
     || force_init
     ) {
    local bOpenNowloadingMyself = !::g_menu_nowloading.is_open() && ("" != rom);

    if (bOpenNowloadingMyself) {
      ::g_menu_nowloading.setCoord(0,0);
//      ::g_menu_nowloading.open(NowLoadingTypes.NOW_LOADING, null);
    }

    // エミュレータ初期化の再入を防ぐ
    while(!::g_emu_task.initialized) 
      wait(0);

    local bu_visible = ::g_emu_task.visible;
    local bu_pause   = ::g_emu_task.pause;

    ::g_emu_task.visible = false;
    ::g_emu_task.pause   = true;

    // emulator 再初期化
    ret = ::init_emulator(regionTag);

    // （emuの切り替わりを待つ）
    if (!::g_emu_task.initialized) {
      local dialog = null;
      if (noticeKey) {
        dialog = NoticeDialog();
        dialog.init();
        dialog.open(noticeKey, "GuideBody__NULL_DUMMY");
      }

      while (!::g_emu_task.initialized) 
        wait(0);

      if (dialog) 
        dialog.close();
    }

    if (bOpenNowloadingMyself) 
    {
//      ::g_menu_nowloading.close();
    }

    // 初期化成功？
    if (! ::g_emu_task.initializeSuccess) {
      // 失敗してたら、次の再初期化要求が必ず通るようにしておく
      s_init_emulator_regionTag_current = null;
      s_init_emulator_rom_current = null;
    }

    ::g_emu_task.visible = bu_visible;
    ::g_emu_task.pause   = bu_pause;
  }

  return ret;
}

// -------------------------------------------
// アレンジサウンド設定
// -------------------------------------------
function set_emulator_arrange_sound()
{
  if(!::g_emu_task.initialized) 
    {
      ::g_menu_nowloading.setCoord(0,0);
//      ::g_menu_nowloading.open(NowLoadingTypes.NOW_LOADING, null);
      while (!::g_emu_task.initialized) 
        wait(0);
//      ::g_menu_nowloading.close();
    }

  // ----------- アレンジサウンド用追加
  // arrange sound設定
  local title_prof = ::get_current_title_prof();
  if ("arrange_sound" in title_prof) 
    {
      local arrange_sound = title_prof.arrange_sound;
      local volume = 0.7;        //default
      if ("volume" in arrange_sound) 
        {
          volume = arrange_sound.volume;
        }
      ::g_emu_task.setArrSoundVolume(volume);
      if ("track_data" in arrange_sound) 
        {
          local track_data = arrange_sound.track_data;
          foreach (val in track_data) 
            {
              local req = val.req;
              local trk = val.track;
              ::g_emu_task.setArrSoundData(req,trk);
            }
        }
      if ("secmd_mask" in arrange_sound) 
        {
          local mask = arrange_sound.secmd_mask;
          local req = arrange_sound.secmd_req;
          ::g_emu_task.setArrSoundSeMask(mask,req);
        }
      if ("fadeout_time" in arrange_sound) 
        {
          local cnt = arrange_sound.fadeout_time;
          ::g_emu_task.setArrSoundFadeoutTime(cnt);
        }
      if ("multibyte_cmd" in arrange_sound) 
        {
          local multicmd = arrange_sound.multibyte_cmd;
          foreach (val in multicmd) 
            {
              local req = val.req;
              local cmd = val.cmd;
              ::g_emu_task.setArrSoundMultiCmdSet(req,cmd);
            }
        }
    }
  // ----------- アレンジサウンド用　ここまで

}

// -------------------------------------------
// emulator リセット
// -------------------------------------------
s_reset_emulator_request <- null;
s_reset_emulator_count <- null;

// reset_emulator()の後処理
// 基本的にはreset_emulator()内部で呼ばれる
function reset_emulator_after()
{
  s_reset_emulator_request = false;
  s_reset_emulator_count ++;
}

// emulatorをリセットする
function reset_emulator(_hot_reset = null)
{
  printf("\n\treset emulator (%s)\n\n", _hot_reset);

  if (!::g_emu_task.initialized) {
    ::g_menu_nowloading.setCoord(0,0);
//    ::g_menu_nowloading.open(NowLoadingTypes.NOW_LOADING, null);
    while (!::g_emu_task.initialized) 
      wait(0);
//    ::g_menu_nowloading.close();
  }

  ::g_emu_task.reset(_hot_reset != null);

  if (::g_systemdata) {
    ::g_emu_task.sram = ::g_systemdata.get_data_setting_savedata__emu_buram();
  }

  set_emulator_arrange_sound();   // アレンジサウンド設定
  ::g_emu_task.setMemoryPatch(1); // メモリパッチを即時実行
  reset_emulator_after();         // reset_emulator()の後処理
}

// emulatorリセット要求
function request_reset_emulator()
{
  s_reset_emulator_request = true;
}

// emulatorリセットを試みる
// emulatorリセット要求があればreset_emulator()を実行する
function try_reset_emulator(_hot_reset = null)
{
  if (s_reset_emulator_request) {
    reset_emulator(_hot_reset);
    return true;
  }
  else {
    return false;
  }
}

// emulatorリセットカウント初期化
function init_reset_emulator_count()
{
  s_reset_emulator_count = 0;
}

// emulatorリセットカウント取得
function get_reset_emulator_count()
{
  return s_reset_emulator_count;
}


// -------------------------------------------
// リプレイ記録/再生
// -------------------------------------------

// リプレイ記録 開始
function start_replay_record( _record_minutes = null, _ignore_medal_system_work = false )
{
  local record_minutes = 0;
  if ( null != _record_minutes ) {
    record_minutes = _record_minutes;
  }
  else {
    if (::g_systemdata.get_data_setting_savedata__record_replay()) {
      record_minutes = MINUTES_REPLAY_RECORD;
      local title_prof = get_current_title_prof();
      if ("max_replay_record_time" in title_prof.replay) {
        record_minutes = title_prof.replay["max_replay_record_time"];
      }
    }
  }

  local max_frame = 60 * 60 * record_minutes;

  printf("------- startRecordReplay() %d frames (= %d minutes)\n", max_frame, max_frame / 60 / 60);
  if (max_frame != 0) {
    //!!!オプションで設定した記録時間を設定する
    local replay_struct = get_backup_struct(BackupSegmentTypes.EMU_REPLAY);
    if (replay_struct) {
      replay_struct.clear();
      ::g_emu_task.startRecordReplay(replay_struct, max_frame);
      if ( ! _ignore_medal_system_work ) {
        ::get_medal_system_1game_work_to(replay_struct.root._030_medalsystem_1gamework);
        ::get_medal_system_backup_work_to(replay_struct.root._040_medalsystem_backupwork);
        ::get_medal_system_common_work_to(replay_struct.root._045_medalsystem_commonwork);
      }
    }
  }

  return (::g_emu_task.replayMode == "record");
}

function is_replay_recording(_emu_task)
{
  return (_emu_task.replayMode == "record");
}

// リプレイ再生 開始
function start_replay_play( _ignore_medal_system_work = false, _ignore_state = false )
{
  local replay_struct = get_backup_struct(BackupSegmentTypes.EMU_REPLAY);
  local res = ::g_emu_task.startPlayReplay(replay_struct, _ignore_state);

  printf("------- startPlayReplay() %d frames\n", ::g_emu_task.replayExpectedRestFrame);
  if (res) {
    if ( ! _ignore_medal_system_work ) {
      ::set_medal_system_1game_work_from(replay_struct.root._030_medalsystem_1gamework);
      ::set_medal_system_backup_work_from(replay_struct.root._040_medalsystem_backupwork); // リプレイ再生後にsystemdataの値で書き戻される
      ::set_medal_system_common_work_from(replay_struct.root._045_medalsystem_commonwork); // リプレイ再生後にsystemdataの値で書き戻される
    }
    /*
    set_emulator_arrange_sound(); // アレンジサウンド登録
    reset_emulator_after();       // reset_emulator()の後処理
    */ 
  }
  else {
    print("unserializeReplay() failed ...\n");
  }

  return (::g_emu_task.replayMode == "play");
}

// リプレイ記録/再生 終了
function stop_replay()
{
  ::g_emu_task.stopReplay();

  print("------- stopReplay()\n");
}




// ----

function _load_emu_binary_with_wait(_path)
{
  local b_compression = true;
  local res = ::g_emu_task.setup_load_emu_binary(_path, b_compression);
  if (!res) 
    return false;

  while (!::g_emu_task.success_load_emu_binary()) 
    wait(0);

  return true;
}

// -------------------------------------------
// emulatorにStateDataイメージを渡す
// （完了するまでブロックする）
// 成功した場合、reset_emulator()と同等のリセット処理が行われる
// conv_path()はコチラでやるので注意！
// -------------------------------------------
function setup_emulator_statedata(statedata_image_path, use_systemdata_regionTag = false)
{
  local ret = false;
  local res;

  if (!::g_emu_task || !::g_emu_task.pause) 
    return ret;

  if (!statedata_image_path || !statedata_image_path.len()) 
    return ret;

  while (!::g_emu_task.initialized) 
    wait(0);

  // バイナリ読み込み
  if (!::_load_emu_binary_with_wait(conv_path(statedata_image_path)))
    return ret;


  // backupのstructと処理を流用するよ
  local backupStatedata = BackupStatedata(null, false); // エラー表示なし

  local struct_statedata = backupStatedata.get_struct();
  struct_statedata.clear();

  // structにバイナリをコピー
  res = ::g_emu_task.copy_load_emu_binary_2_struct(struct_statedata);

  if (res) {
    local game_exit = false;

    // struct内のheaderチェックを行う
    res = backupStatedata.check_header(struct_statedata, true);
    if (res) {
      // emu_taskにStateDataを反映する
      res = backupStatedata.to_emu(struct_statedata, false, false, use_systemdata_regionTag);
      if (res) 
        ret = true;
    }

    // structのメモリ解放
    backupStatedata.purge_struct();
  }

  printf("setup_emulator_statedata() : %s\n", ret ? "success !!" : "failed ...");

  return ret;
}

// -------------------------------------------
// emulatorのStateDataイメージをバイナリでファイル出力する
// （完了するまでブロックする）
// 
// 基本的に「通信プレイ用の頭出しステートデータ」イメージを
// 作成するためのモノです。
// テストメニューでのみ呼び出してください。
// -------------------------------------------
s_last_host_save_emulator_statedata_tick <- null;
s_last_host_save_emulator_statedata_filename <- null;
s_last_host_load_emulator_statedata_filename <- null;

function _get_rom_name_for_host()
{
  if (s_rom_fileparse && s_rom_fileparse.len() > 0) {
    local except_exts = [
                         // ".ccf",
                         ];
    local rom_ext = s_rom_fileparse.ext;
    for (local no = 0; no < except_exts.len(); no++) {
      if (rom_ext == except_exts[no]) {
        rom_ext = "";
        break;
      }
    }
    return s_rom_fileparse.name + rom_ext;
  }
  else {
    return "image";
  }
}

function _get_filename_with_timestamp_for_host(_body_name, _rom_name, _ext_name, _tick)
{
  local filename;
  if ( null == _tick ) {
    filename = format("%s_%s%s", _body_name, _rom_name, _ext_name);
  }
  else {
    local clock = SystemEtc.getClockLocalTime(_tick["HI"], _tick["LW"]);
    filename = format("%s_%s_%04d%02d%02d_%02d%02d%02d%s"
                      , _body_name
                      , _rom_name
                      , clock["year"]
                      , clock["month"]
                      , clock["day"]
                      , clock["hour"]
                      , clock["minute"]
                      , clock["second"]
                      , _ext_name );
    // filename = format("%s_%s_%08x_%08x%s", _body_name, _rom_name, _tick["HI"], _tick["LW"], _ext_name);
  }

  return filename;
}

function host_save_emulator_statedata()
{
  if (!::g_emu_task || !::g_emu_task.pause) 
    return false;

  while (!::g_emu_task.initialized) 
    wait(0);

  // backupのstructと処理を流用するよ
  local backupStatedata = BackupStatedata(null, false); // エラー表示なし

  // バイナリ書き出し（ファイル出力）
  local struct_state = backupStatedata.get_struct();
  struct_state.clear();

  // struct内のheaderを設定する
  backupStatedata.set_statedata_struct(struct_state);

  local tick = SystemEtc.getCurrentTick();
  local filename = ::_get_filename_with_timestamp_for_host("state", ::_get_rom_name_for_host(), ".bin", tick);
  local res = ::g_emu_task.host_save_statedata_image(struct_state, filename);
  s_last_host_save_emulator_statedata_tick = tick;
  s_last_host_save_emulator_statedata_filename = filename;
  printf("host_save_emulator_statedata() : %s\n", res ? "success !!" : "failed ...");
  return res;
}

function host_load_emulator_statedata(_select_by_list = true)
{
  if (!::g_emu_task || !::g_emu_task.pause) 
    return false;

  while (!::g_emu_task.initialized) 
    wait(0);

  // backupのstructと処理を流用するよ
  local backupStatedata = BackupStatedata(null, false); // エラー表示なし
  local struct_state = backupStatedata.get_struct();
  struct_state.clear();

  local filename = "";
  local tick = s_last_host_save_emulator_statedata_tick;
  if (_select_by_list) {
    filename = ::_host_load_select_filename_by_ui();
    if (null == filename) {
      return false;
    }
  }
  else {
    filename = ::_get_filename_with_timestamp_for_host("state", ::_get_rom_name_for_host(), ".bin", null);
  }
  local res = ::g_emu_task.host_load_statedata_image(struct_state, filename);
  if ( !res && null != tick ) {
    filename = ::_get_filename_with_timestamp_for_host("state", ::_get_rom_name_for_host(), ".bin", tick);
    res = ::g_emu_task.host_load_statedata_image(struct_state, filename);
  }
  s_last_host_load_emulator_statedata_filename = filename;
  printf("host_load_emulator_statedata(%s) : %s\n", filename, res ? "success !!" : "failed ...");
  return res;
}

s_last_host_load_select_file_name_by_ui <- null;

function _host_load_select_filename_by_ui()
{
  local select_file = null;
  local load_lists = [];
  local file_lists = ::g_emu_task.host_listup_files(".");
  local base_name = ::_get_rom_name_for_host();
    
  if (null != file_lists) {
    for (local no = 0; no < file_lists.len(); no++ ){
      if ( file_lists[no].find(base_name) != null ) {
        load_lists.append(file_lists[no]);
      }
    }
    if ( 0 < load_lists.len() ) {
      ::util_load_script("system/script/mode_select_list.nut");
      local menu = ModeSelectList(load_lists, s_last_host_load_select_file_name_by_ui);
      select_file = menu.exec();
      if ( null != select_file ) {
        s_last_host_load_select_file_name_by_ui = select_file;
      }
    }
  }
  return select_file;
}

function get_last_host_save_emulator_statedata_filename()
{
  return s_last_host_save_emulator_statedata_filename;
}

function get_last_host_load_emulator_statedata_filename()
{
  return s_last_host_load_emulator_statedata_filename;
}

// -------------------------------------------
// ReplayDataイメージを読み込む
// （完了するまでブロックする）
// 成功した場合、reset_emulator()と同等のリセット処理が行われる
// conv_path()はコチラでやるので注意！
// -------------------------------------------
function setup_emulator_replaydata(replaydata_image_path)
{
  local ret = false;

  if (!::g_emu_task || !::g_emu_task.pause || !replaydata_image_path || !replaydata_image_path.len()) 
    return ret;

  while (!::g_emu_task.initialized) 
    wait(0);

  // バイナリ読み込み
  if (!::_load_emu_binary_with_wait(conv_path(replaydata_image_path)))
    return ret;

  local struct_replay = get_backup_struct(BackupSegmentTypes.EMU_REPLAY);
  if (null != struct_replay) {
    struct_replay.clear();
    local res = ::g_emu_task.copy_load_emu_binary_2_struct(struct_replay);

    if (res) {
      local game_exit = false;
      res = false;
      do {
        // 構造体バージョンチェック
        if (struct_replay.root._00_header._00_version > BackupStructVersion.EMU_REPLAY) {
          printf("header.version : %08x\n", struct_replay.root._00_header._00_version);
          break;
        }
            
        // StateDataバージョンチェック(EmuVersion)
        if (struct_replay.root._00_header._01_data_version != ::g_emu_task.getEmuVersion()) {
          printf("_00_header._01_data_version : %08x\n", struct_replay.root._00_header._01_data_version);
          break;
        }
            
        // Emu Countryが違っていた場合はemulator再初期化
        if (struct_replay.root._01_added_value._06_region != get_last_init_emulator_region()) {
          printf("_01_added_value._06_region : %s\n", struct_replay.root._01_added_value._06_region);
          request_re_init_emulator(struct_replay.root._01_added_value._06_region); // emulator 再初期化 リクエスト
          re_init_emulator(); // emulator 再初期化
          game_exit = session_wait_check();
          if (game_exit)
            break;
        }
            
        // reset_emulator()の後処理
        reset_emulator_after();
        ret = true;
        break;

      } while (1);

      if (game_exit) 
        return ret;
    }
  }

  printf("setup_emulator_replaydata() : %s\n", ret ? "success !!" : "failed ...");

  return ret;
}

// -------------------------------------------
// emulatorのReplayDataイメージをバイナリでファイル出力する
// （完了するまでブロックする）
// テストメニューでのみ呼び出してください。
// -------------------------------------------
s_last_host_save_emulator_replaydata_filename <- null;
s_last_host_load_emulator_replaydata_filename <- null;

function host_save_emulator_replaydata(_no_set_header = false)
{
  if (!::g_emu_task || !::g_emu_task.pause) 
    return;

  while (!::g_emu_task.initialized) 
    wait(0);

  if ( !_no_set_header )
    ::set_replay_data_header();

  local struct_replay = get_backup_struct(BackupSegmentTypes.EMU_REPLAY);
  local filename = "replay_" + (s_rom_fileparse && s_rom_fileparse.len() > 0 ? s_rom_fileparse.name : "image") + ".bin";
  s_last_host_save_emulator_replaydata_filename = filename;
  local res = ::g_emu_task.host_save_sqstruct_image(struct_replay, filename);
  printf("host_save_emulator_replaydata() : %s\n", res ? "success !!" : "failed ...");
  return res;
}

function host_load_emulator_replaydata()
{
  if (!::g_emu_task || !::g_emu_task.pause) 
    return;

  while (!::g_emu_task.initialized) 
    wait(0);

  local struct_replay = get_backup_struct(BackupSegmentTypes.EMU_REPLAY);
  local filename = "replay_" + (s_rom_fileparse && s_rom_fileparse.len() > 0 ? s_rom_fileparse.name : "image") + ".bin";
  s_last_host_load_emulator_replaydata_filename = filename;
  local res = ::g_emu_task.host_load_sqstruct_image(struct_replay, filename);
  printf("host_load_emulator_replaydata() : %s\n", res ? "success !!" : "failed ...");
  return res;
}

function get_last_host_save_emulator_replaydata_filename()
{
  return s_last_host_save_emulator_replaydata_filename;
}

function get_last_host_load_emulator_replaydata_filename()
{
  return s_last_host_load_emulator_replaydata_filename;
}

// -------------------------------------------
// 画面イメージをbmpファイル出力する
// （完了するまでブロックする）
// テストメニューでのみ呼び出してください。
// -------------------------------------------
function host_save_screenshot( _capture )
{
  if (!::g_emu_task) 
    return;

  while (!::g_emu_task.initialized) 
    wait(0);

  local rsc = Resource();
  local struct_pic_path = ::conv_path( "system/config/struct_picture_bitmap.psb");
  rsc.load(struct_pic_path);
  while (rsc.loading) {
    wait(0);
  }
  local struct_pic = Struct( rsc.find(struct_pic_path) );
  local tick = SystemEtc.getCurrentTick();
  local filename = ::_get_filename_with_timestamp_for_host("screenshot", ::_get_rom_name_for_host(), ".bmp", tick);

  local bounds = System.getScreenBounds();
  local w = bounds.width;
  local h = bounds.height;
  local header_size = 14;
  local info_size   = 40;
  local pitch = ((w*3 + (4-1))/4).tointeger()*4;
  local image_size  = pitch * h;
  local size = header_size + info_size + image_size;
  local type = 0x28;
  local bit_count = 24;
  local header = struct_pic["root"]["_00_header"];
  header["_00_type"][0] = 'B';
  header["_00_type"][1] = 'M';
  header["_01_size"]    = size;
  header["_04_offset"]  = header_size + info_size;
  local info = struct_pic["root"]["_01_info"];
  info["_00_type"]      = type; 
  info["_01_width"]     = w;
  info["_02_height"]    = -h;
  info["_03_planes"]    = 1;
  info["_04_bitCount"]  = bit_count;
  info["_05_compress"]  = 0;
  if ( BinaryUtil.StoreBitmapImage(_capture, struct_pic["root"]["_02_image"], w, h, pitch) ) {
    local res = ::g_emu_task.host_save_sqstruct_image(struct_pic, filename);
    printf("host_save_screenshot() : %s\n", res ? "success !!" : "failed ...");
  }
}

// -------------------------------------------
// emulator ワークラムチェック
// -------------------------------------------

function checkStateValue(item)
{
  local address = item.address & 0xffff;
  local stat = null;

  if( address == null )
    {
      return( false );
    }

  //文字列データ
  if( item.mode == "string" )
    {
      foreach( comp in item.value )
        {
          //比較データのサイズ分、メモリから文字列を作成する
          for( local i = 0 ; i < comp.len() ; i++ )
            {
              if( i == 0 )
                {
                  stat =   format("%c",::g_emu_task.getRamValue(8, address + i));
                }
              else
                {
                  stat +=  format("%c",::g_emu_task.getRamValue(8, address + i));
                }
            }

          if( stat == null )
            {
              return( false );
            }

          //printf("stat %s comp %s\n",stat,comp);

          if( stat == comp )
            {
              return( stat );
            }
        }

      return( false );
    }
  else
    //数値データ
    {
      switch( item.mode )
        {
          //バイトデータ
        case "byte":
          stat = ::g_emu_task.getRamValue(8, address);
          break;
          //ワードデータ
        case "word":
          stat = ::g_emu_task.getRamValue(16, address);
          break;
          /*
          //ロングデータ
          case "long":
          stat = ::g_emu_task.getRamValue(32, address);
          break;
          */
        }
    }

  {
    if( stat == null )
      {
        return( false );
      }

    foreach( comp in item.value )
      {
        //printf("stat %x comp %x\n",stat,comp);
        if( stat == comp )
          {
            return( stat );
          }
      }
  }

  return( false );
}


// -------------------------------------------
// emulator ワークラム値設定
// -------------------------------------------

function setStateValue(item)
{
  local address = item.address & 0xffff;
  local stat = null;

  if( address == null )
    {
      return( false );
    }

  //文字列データ
  if( item.mode == "string" )
    {
      return;
    }
  else
    //数値データ
    {
      switch( item.mode )
        {
          //バイトデータ
        case "byte":
          foreach( val in item.value )
            ::g_emu_task.setRamValue(8, address, val);
          break;
          //ワードデータ
        case "word":
          foreach( val in item.value )
            ::g_emu_task.setRamValue(16, address, val);
          break;
          /*
          //ロングデータ
          case "long":
          stat = ::g_emu_task.getRamValue(32, address);
          break;
          */
        }
    }
}


//indicator指定時は判定用ステータス情報を表示する
function getStateCharId(state_check_char,player,indicator)
{
  local chara_id = null;

  local stat0 = null;    //ゲームモードステータス判定用
  local stat1 = null;    //１Ｐ動作モード判定用
  local stat2 = null;    //２Ｐ動作モード判定用

  {
    local chk0 = false;
    local chk1 = false;
    local chk2 = false;

    //システムステータスチェック
    {
      {
        if( state_check_char.system != "" )
          {
            if( state_check_char.system.stat != "" )
              {
                foreach( item in state_check_char.system.stat )
                  {
                    local stat = checkStateValue(item);
                    if( stat == false )
                      {
                        stat0 = false;
                        chk0 = false;
                        break;
                      }
                    // 一致
                    stat0 = stat;
                    chk0 = true;
                  }
              }
          }
      }
    }

    //1Pステータスチェック
    if
      (
       ( stat0 == null )
       ||    ( chk0 == true )
       ) {
      if ( player == 0 ) {
        if ( state_check_char.p1 != "" ) {
          if ( state_check_char.p1.stat != "" ) {
            foreach( item in state_check_char.p1.stat ) {
              local stat = checkStateValue(item);
              if ( stat == false ) {
                stat1 = false;
                chk1 = false;
                break;
              }
              stat1 = stat;
              chk1 = true;
            }
          }

          if
            (
             ( state_check_char.p1.chara )
             &&    (
                    ( stat1 == null )
                    ||  ( chk1 == true )
                    )
             ) {
            foreach( item in state_check_char.p1.chara ) {
              local stat = checkStateValue(item);
              if ( stat != false ) {
                chara_id = stat;
                break;
              }
            }
          }
        }
      }
      //2Pステータスチェック
      else {
        if ( state_check_char.p2 != "" ) {
          if ( state_check_char.p2.stat != "" ) {
            foreach( item in state_check_char.p2.stat ) {
              local stat = checkStateValue(item);
              if ( stat == false ) {
                stat2 = false;
                chk2 = false;
                break;
              }
              stat2 = stat;
              chk2 = true;
            }
          }

          if
            (
             ( state_check_char.p2.chara )
             &&    (
                    ( stat2 == null )
                    ||  ( chk2 == true )
                    )
             ) {
            foreach( item in state_check_char.p2.chara ) {
              local stat = checkStateValue(item);
              if ( stat != false ) {
                chara_id = stat;
                break;
              }
            }
          }
        }
      }
    }
  }

  if ( ::isDebugBuild() ) {
    local prn = "";

    if ( stat0 == false ) {
      prn += format("stat0 false\n");
    }
    else if ( stat0 != null ) {
      prn += format("stat0 %02x\n",stat0);
    }

    if ( stat1 == false ) {
      prn += format("stat1 false\n");
    }
    else if ( stat1 != null ) {
      if ((typeof stat1) == "integer") {
        prn += format("stat1 %02x\n",stat1);
      }
      else {
        prn += format("stat1 %s\n",stat1);
      }
    }

    if ( stat2 == false ) {
      prn += format("stat2 false\n");
    }
    else if ( stat2 != null ) {
      if ((typeof stat2) == "integer") {
        prn += format("stat2 %02x\n",stat2);
      }
      else {
        prn += format("stat2 %s\n",stat2);
      }
    }

    if ( chara_id != null ) {
      prn += format("chara_id %02x\n",chara_id);
    }


    if ( indicator != null ) {
      indicator.print(prn);
    }
    else {
      print( prn );
    }
  }

  return( chara_id );
}


// -------------------------------------------
// OriginalBug 対応チェック
// -------------------------------------------
function getOriginalBugMode()
{
  local title_prof = ::get_current_title_prof();
  if ( !("original_bug" in title_prof) 
       || !("mode" in title_prof.original_bug) 
       || (title_prof.original_bug.mode == 0)
       ) {
    return 0;
  }

  return title_prof.original_bug.mode;
}

// -------------------------------------------
// OriginalBug 書き換えアドレス、値取得
// -------------------------------------------
function setOriginalBugData()
{
  s_last_apply_emulateOriginalBug = false;

  if ( !getOriginalBugMode() )
    return false;
  
  local title_prof = ::get_current_title_prof();
  if ( !("original_bug" in title_prof)
       || !("set_list" in title_prof.original_bug)
       ) 
    return false;

  local emulate = ::g_systemdata.get_data_setting_etc__org_bug_emulate();
  if (s_hook_emulateOriginalBug != null) 
    emulate = s_hook_emulateOriginalBug;

  printf("### original bug ");

  foreach ( list in title_prof.original_bug.set_list ) {
    if ( ! ("offset" in list)
         || !("data" in list)
         )
      continue;

    local offset = list.offset & 0x0000ffff;
    local data   = list.data;

    // オリジナルバグ オフ
    if ( !emulate ) {
      data = 0x00;
    }

    ::g_emu_task.setRamValue(8, offset, data );

    printf("0x%08x -> 0x%02x ", offset, data);
  }

  printf("###\n");

  s_last_apply_emulateOriginalBug = emulate;

  return true;
}

// -------------------------------------------
// OriginalBug フック
// -------------------------------------------
s_hook_emulateOriginalBug <- null;
s_last_apply_emulateOriginalBug <- null;

// nullでhook解除
function hookEmulateOriginalBug(is_emulate=null)
{
  s_hook_emulateOriginalBug = is_emulate;

  printf(">>>s_hook_emulateOriginalBug");
  if (s_hook_emulateOriginalBug == null) {
    printf(" null");
  }
  else {
    printf(" %d", s_hook_emulateOriginalBug);
  }
  printf("\n");
}

function get_last_requestEmulateOriginalBug()
{
  if (s_last_apply_emulateOriginalBug == null) 
    s_last_apply_emulateOriginalBug = false;

  printf(">>>s_last_apply_emulateOriginalBug %d\n", s_last_apply_emulateOriginalBug);

  return s_last_apply_emulateOriginalBug;
}

// -------------------------------------------
// デフォルトパッチ 書き換えアドレス、値取得
// -------------------------------------------
function setDefaultPatch()
{
  local title_prof = ::get_current_title_prof();

  if ( !("default_patch" in title_prof)
       || !("mode" in title_prof.default_patch)
       || (title_prof.default_patch.mode == 0)
       || !("set_list" in title_prof.default_patch)
       )
    return false;

  printf("### default patch ");

  foreach( list in title_prof.default_patch.set_list )  {
    if ( !("offset" in list)
         || !("data" in list)
         )
      continue;

    local offset = list.offset & 0x0000ffff;
    local data   = list.data;

    ::g_emu_task.setRamValue(8, offset, data );

    printf("0x%08x -> 0x%02x ", offset, data);
  }

  printf("###\n");

  return true;
}
