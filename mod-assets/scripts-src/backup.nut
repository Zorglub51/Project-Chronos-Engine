// 
// バックアップ
// 

// -----------------------------------------------------------------------
// BackupControl クラス
// 
// backup処理が必要になった時点でインスタンスを生成/初期化して、
// backup処理完了後に解放してください。
// -----------------------------------------------------------------------

class BackupControlBase {
  backup_manager_system = null;
  backup_manager_title = null;
  struct_table = null;
  segment_table = null;

  current_title_name = null;	// title_name
  current_series_name = null; // series_name

  // captured icon0
  capture = null;

  b_initialized = null;

  codeList = null;

  static CodeListBase = ["", null, "\x0a", "%d", "%02d", "%03d"];

  // struct内部メモリの解放許可フラグ
  static can_purge_struct = true;

  m_rsc = null;
  m_savedata_string = null;
  m_title_string    = null;
  m_disable_full_operation = false;

  // ----------------------------------
  // コンストラクタ
  // ----------------------------------
  constructor() {
    local savedata_string_path   = conv_path(this.get_save_data_string_path());
    local title_save_string_path = conv_path(CONFIG_TITLE_SAVE_STRING_PATH);
    m_rsc = Resource();
    m_rsc.load(savedata_string_path, title_save_string_path);
    while (m_rsc.loading)
      wait(0);

    m_savedata_string = m_rsc.find(savedata_string_path).root;   // Segmentにポインタとして設定するため、instanceとして保持
    m_title_string    = m_rsc.find(title_save_string_path).root; // Segmentにポインタとして設定するため、instanceとして保持

    struct_table = {};
    segment_table = {};

    b_initialized = false;

    this._create_struct();
  }

  // ----------------------------------
  // 
  // ----------------------------------
  function _create_struct() {
    local struct_dummy_path    = conv_path(STRUCT_SAVEDATA_DUMMY_PATH);
    local struct_system_path   = conv_path(STRUCT_SYSTEMDATA_PATH);
    local struct_title_path    = conv_path( ::is_package_containing_multi_title() ? STRUCT_SYSTEMDATA_TITLES_PATH : STRUCT_SYSTEMDATA_TITLE_PATH );
    local struct_playlist_path = conv_path(STRUCT_SOUND_PLAY_LIST_PATH);
    local struct_state_path    = conv_path(STRUCT_STATEDATA_PATH);
    local struct_state_path_l    = conv_path(STRUCT_STATEDATA_L_PATH);
    local struct_replay_path   = conv_path(STRUCT_REPLAYDATA_PATH);

    // 必要なリソースをロード
    local tmp_rsc = Resource();
    tmp_rsc.load(
                 struct_dummy_path,
                 struct_system_path,
                 struct_title_path,
                 struct_playlist_path,
                 struct_state_path,
                 struct_state_path_l,
                 struct_replay_path
                 );
    while (tmp_rsc.loading)
      wait(0);

    struct_table[-1                                      ] <- Struct(tmp_rsc.find(struct_dummy_path));
    struct_table[BackupSegmentTypes.SYSTEM_DATA          ] <- Struct(tmp_rsc.find(struct_system_path));
    struct_table[BackupSegmentTypes.SOUND_PLAY_LIST      ] <- Struct(tmp_rsc.find(struct_playlist_path));
    struct_table[BackupSegmentTypes.TITLE_DATA           ] <- Struct(tmp_rsc.find(struct_title_path));
    struct_table[BackupSegmentTypes.TITLE_SOUND_PLAY_LIST] <- Struct(tmp_rsc.find(struct_playlist_path));
    struct_table[BackupSegmentTypes.EMU_STATE            ] <- Struct(tmp_rsc.find(struct_state_path));
    struct_table[BackupSegmentTypes.EMU_STATE_L          ] <- Struct(tmp_rsc.find(struct_state_path_l));
    struct_table[BackupSegmentTypes.EMU_REPLAY           ] <- Struct(tmp_rsc.find(struct_replay_path));

    struct_table[BackupSegmentTypes.EMU_REPLAY].clear();
  }

  // ----------------------------------
  // 
  // ----------------------------------
  function create_struct(path) {
    local struct_path = conv_path(path);

    // 必要なリソースをロード
    local tmp_rsc = Resource();
    tmp_rsc.load(struct_path);
    while (tmp_rsc.loading)
      wait(0);

    return Struct(tmp_rsc.find(struct_path));
  }

  // ----------------------------------
  // 初期化済みチェック
  // ----------------------------------
  function is_init() {
    return b_initialized;
  }

  // ----------------------------------
  // backup_manager_titleに設定されたtitleIdかチェックする
  // (同一titleでpackageRegion毎にtitleIdが違うケースも考慮する)
  // ----------------------------------
  function check_titleId(titleId) {
    local ret = false;

    if (!b_initialized) return ret;
    if (!backup_manager_title)  return ret;

    local param = get_title_package_param( ::get_package_dev_id() );
    if (param != null) {
      local keyTitleID = "title_id";
      foreach (key, item in param) {
        if ((typeof item) == "table") {
          if (keyTitleID in item) {
            if (item[keyTitleID] == titleId) {
              ret = true;
              break;
            }
          }
        }
      }
    }

    // system_prof(package_prf)にpackage作成情報がまだ無いケース？
    else {
      local regionTags = get_package_regionTag_table();
      foreach (keyRegionTag, val in regionTags) {
        if (get_package_sample_titleID(keyRegionTag) == titleId) {
          ret = true;
          break;
        }
      }
    }

    return ret;
  }

  // ----------------------------------
  // backup_manager_titleに設定されたtitleIdを返す
  // ----------------------------------
  function get_titleId() {
    local ret = null;

    if (!b_initialized)
      return ret;
    if (!backup_manager_title)
      return ret;

    ret = backup_manager_title.titleId;

    return ret;
  }

  // ----------------------------------
  // backup_manager_systemに設定されたtitleIdを返す
  // ----------------------------------
  function get_system_titleId() {
    local ret = null;

    if (!b_initialized)
      return ret;
    if (!backup_manager_system)
      return ret;

    ret = backup_manager_system.titleId;

    return ret;
  }

  // ----------------------------------
  // 初期化
  // ----------------------------------
  function init() {
    local ret = false;

    if (backup_manager_system) 
      backup_manager_system = null;

    if (backup_manager_title) 
      backup_manager_title = null;

    local dev_id = ::get_package_dev_id();
    printf("dev_name : %s\n", get_title_dev_name(dev_id));
    if (dev_id == 1) {
      print("==== this is SystemTitle !! ====\n");
    }

    local package_title = ::get_title_item(dev_id);
    local common_title  = ::get_title_item(1);        // XXX 共有セーブデータはdev_id=1のものを使う

    current_title_name  = get_item_string( package_title["name"] ); // 現在のタイトル名
    current_series_name = get_item_string( get_system_prof().root["series_name"] ); // シリーズ名称

    codeList = [];
    foreach (i, code in CodeListBase) {
      switch (i) {
      case 0:
        code = current_title_name;
        break;
      case 1:
        code = current_series_name;
        break;
      }
      codeList.append( code );
    }

    // システム共通なセーブデータ
    // backup_manager_system 生成＆初期化準備
    // BackupManager生成
    backup_manager_system = this.init_backup_manager(true, common_title);
    backup_manager_system.title = this._get_join_message("system_title", true); 

    backup_manager_system.setEnableMetaData(true);

    // SYSTEM_DATA
    if ( ::is_package_use_common_save_data() ) {
      local id = BackupSegmentTypes.SYSTEM_DATA;
      local segment = backup_manager_system.addSegment(struct_table[id], this.get_backup_segment_num(id));
      segment_table[id] <- segment;
      this.set_segment_info_common_system(segment);
    }
    else
      backup_manager_system.addSegment(struct_table[-1], 0);

    // SOUND_PLAY_LIST
    if ( ::is_package_use_common_save_data() ) {
      local id = BackupSegmentTypes.SOUND_PLAY_LIST;
      local segment_num = this.get_backup_segment_num(id);
      local segment = backup_manager_system.addSegment(struct_table[id], segment_num);
      segment_table[id] <- segment;
      if (segment_num > 0) {
        segment.fileId = ::get_package_sound_play_list_file_id();
      }
      this.set_segment_info_playlist(segment, true);
    }
    else
      backup_manager_system.addSegment(struct_table[-1], 0);

    // タイトル依存なセーブデータ
    // backup_manager_title 生成＆初期化準備
    // BackupManager生成
    backup_manager_title = this.init_backup_manager(false, package_title);
    backup_manager_title.title = this._get_join_message("gametitle_title");

    backup_manager_title.setEnableMetaData(true);

    // system用segment分だけdummyのsegmentを登録
    for (local i=0; i<BackupSegmentTypes.TITLE_DATA; i++) 
      backup_manager_title.addSegment(struct_table[-1], 0);

    // TITLE_DATA
    {
      local id = BackupSegmentTypes.TITLE_DATA;
      local segment = backup_manager_title.addSegment(struct_table[id], this.get_backup_segment_num(id));
      segment_table[id] <- segment;
      this.set_segment_info_title_system(segment);
    }

    // TITLE_SOUND_PLAY_LIST
    {
      local id = BackupSegmentTypes.TITLE_SOUND_PLAY_LIST;
      local segment = backup_manager_title.addSegment(struct_table[id], this.get_backup_segment_num(id));
      segment_table[id] <- segment;
      this.set_segment_info_playlist(segment, false);
    }

    backup_manager_title.addSegment(struct_table[-1], 0); // _DUMMY_10
//    backup_manager_title.addSegment(struct_table[-1], 0); // _DUMMY_11

    local title_num = get_package_containing_title_num();
    for (local title_no = 0; title_no < title_num; title_no++) {

      // EMU_STATE_L
      local id = BackupSegmentTypes.EMU_STATE_L;
      local segment = backup_manager_title.addSegment(struct_table[id], this.get_backup_segment_num(id));
      segment_table[id + NUM_EMU_DATA_SEGMENT_TYPES*title_no] <- segment;
      this.set_segment_info_emu_state(segment, title_no, true);
      segment.recordFileId = true;

      // EMU_STATE
      local id = BackupSegmentTypes.EMU_STATE;
      local segment = backup_manager_title.addSegment(struct_table[id], this.get_backup_segment_num(id));
      segment_table[id + NUM_EMU_DATA_SEGMENT_TYPES*title_no] <- segment;
      this.set_segment_info_emu_state(segment, title_no, true);
      segment.recordFileId = true;

      // EMU_REPLAY
      local id = BackupSegmentTypes.EMU_REPLAY;
      local segment = backup_manager_title.addSegment(struct_table[id], this.get_backup_segment_num(id));
      segment_table[id + NUM_EMU_DATA_SEGMENT_TYPES*title_no] <- segment;
      this.set_segment_info_emu_replay(segment, title_no, true);
      segment.recordFileId = true;
    }

    if (backup_manager_system)  {
      backup_manager_system.init();
      while (backup_manager_system.running) 
        wait(0);
      ret = true;
    }

    if (backup_manager_title) {
      backup_manager_title.init();
      while (backup_manager_title.running) 
        wait(0);
      ret = true;
    }

    b_initialized = ret;

    return ret;
  }

  // ----------------------------------
  // セーブデータ文字列を返す
  // ----------------------------------
  function get_savedata_string() {
    return m_savedata_string;
  }

  function _get_savedata_item_string(_tag) {
    if ( ("savedata" in m_title_string) && (_tag in m_title_string["savedata"]) ) {
      return this._get_savedata_item_string_instance( m_title_string["savedata"][_tag] );
    }
    else {
      local savedata_str = this.get_savedata_string();
      if ( _tag in savedata_str )
        return this._get_savedata_item_string_instance( savedata_str[_tag] );
      else
        return null;
    }
  }

  function _get_savedata_item_string_instance(_table) {
    local langTag = ::sys_get_language_tag();
    if ( !( langTag in _table ) ) {
      langTag = ::get_item_string_default_language();
    }
    return _table[ langTag ];
  }

  // ----------------------------------
  // 該当のbackup_managerのtitle文字列をスリかえる
  // ----------------------------------
  function swap_backup_title(seg_type, b_reset = false) {
    local backup_manager = null;
    local key = null;

    switch (seg_type) {
      // system
    case BackupSegmentTypes.SYSTEM_DATA:
    case BackupSegmentTypes.SYSTEM_ARCHIVE:
    case BackupSegmentTypes.SOUND_PLAY_LIST:
      backup_manager = backup_manager_system;
      key = "system_title";
      break;
      // title

    case BackupSegmentTypes.TITLE_DATA:
    case BackupSegmentTypes.TITLE_SOUND_PLAY_LIST:
    case BackupSegmentTypes.EMU_STATE_L:
    case BackupSegmentTypes.EMU_STATE:
    case BackupSegmentTypes.EMU_REPLAY:
      backup_manager = backup_manager_title;
      key = "gametitle_title";
      break;

    default:
      break;
    }

    if (!backup_manager)
      return;

    if (b_reset == false)  {
      switch (seg_type) {
        // system
      case BackupSegmentTypes.SYSTEM_DATA:
        key = "system_savedata_title";
        break;
      case BackupSegmentTypes.SYSTEM_ARCHIVE:
        key = "system_archive_title";
        break;
      case BackupSegmentTypes.SOUND_PLAY_LIST:
        key = "sound_play_list_title";
        break;

        // title
      case BackupSegmentTypes.TITLE_DATA:
        key = "title_savedata_title";
        break;
      case BackupSegmentTypes.TITLE_SOUND_PLAY_LIST:
        key = "sound_play_list_title";
        break;
      case BackupSegmentTypes.EMU_STATE_L:
        key = "statedata_title";
        break;
      case BackupSegmentTypes.EMU_STATE:
        key = "statedata_title";
        break;
      case BackupSegmentTypes.EMU_REPLAY:
        key = "replaydata_title";
        break;

      default:
        break;
      }
    }

    if (key) {
      backup_manager.title = this._get_join_message(key);
    }
  }

  // ----------------------------------
  // backup_segment取得
  // ----------------------------------
  function get_backup_segment(seg_type) {
    if ( ::is_package_containing_multi_title() ) {
      switch (seg_type) {
      case BackupSegmentTypes.EMU_STATE_L:
      case BackupSegmentTypes.EMU_STATE:
      case BackupSegmentTypes.EMU_REPLAY:
        // これらは複数まとめパックの場合、タイトル毎に別のセグメント
        seg_type += ::get_current_title_in_pack_index() * NUM_EMU_DATA_SEGMENT_TYPES;
        break;
      }
    }

    if (seg_type in segment_table) 
      return segment_table[seg_type];

    return null;
  }

  function get_backup_seg_type(_segment) {
    foreach (key, val in segment_table) {
      if ( val == _segment ) {
        return key;
      }
    }
    return null;
  }

  // ----------------------------------
  // backup_struct取得
  // ----------------------------------
  function get_backup_struct(seg_type) {
    if (seg_type in struct_table) 
      return struct_table[seg_type];

    return null;
  }

  // ----------------------------------
  // セーブデータType毎のデータ個数を取得
  // ----------------------------------
  function get_backup_segment_num(seg_type) {
    local num = 1;

    switch (seg_type) {
      // system
    case BackupSegmentTypes.SYSTEM_DATA:
      num = BackupSegmentNum.SYSTEM_DATA;
      break;
    case BackupSegmentTypes.SYSTEM_ARCHIVE:
      num = BackupSegmentNum.SYSTEM_ARCHIVE;
      break;
    case BackupSegmentTypes.SOUND_PLAY_LIST:
      num = BackupSegmentNum.SOUND_PLAY_LIST;
      break;

      // title
    case BackupSegmentTypes.TITLE_DATA:
      num = BackupSegmentNum.TITLE_DATA;
      break;
    case BackupSegmentTypes.TITLE_SOUND_PLAY_LIST:
      num = BackupSegmentNum.TITLE_SOUND_PLAY_LIST;
      break;
    case BackupSegmentTypes.EMU_STATE:
      num = BackupSegmentNum.EMU_STATE;
      break;
    case BackupSegmentTypes.EMU_STATE_L:
      num = BackupSegmentNum.EMU_STATE_L;
      break;
    case BackupSegmentTypes.EMU_REPLAY:
      num = BackupSegmentNum.EMU_REPLAY;
      break;

    default:
      break;
    }

    return num;
  }

  // ----------------------------------
  // セーブデータ構造体のフォーマットバージョン取得
  // ----------------------------------
  function get_backup_struct_version(seg_type) {
    local version = null;

    switch (seg_type) {
      // system
    case BackupSegmentTypes.SYSTEM_DATA:
      version = BackupStructVersion.SYSTEM_DATA;
      break;
    case BackupSegmentTypes.SYSTEM_ARCHIVE:
      version = BackupStructVersion.SYSTEM_ARCHIVE;
      break;
    case BackupSegmentTypes.SOUND_PLAY_LIST:
      version = BackupStructVersion.SOUND_PLAY_LIST;
      break;

      // title
    case BackupSegmentTypes.TITLE_DATA:
      version = BackupStructVersion.TITLE_DATA;
      break;
    case BackupSegmentTypes.TITLE_SOUND_PLAY_LIST:
      version = BackupStructVersion.TITLE_SOUND_PLAY_LIST;
      break;
    case BackupSegmentTypes.EMU_STATE:
      version = BackupStructVersion.EMU_STATE;
      break;
    case BackupSegmentTypes.EMU_STATE_L:
      version = BackupStructVersion.EMU_STATE_L;
      break;
    case BackupSegmentTypes.EMU_REPLAY:
      version = BackupStructVersion.EMU_REPLAY;
      break;

    default:
      break;
    }
    
    return version;
  }

  // ----------------------------------
  // セーブ/ロードの前処理
  // ----------------------------------
  function pre_process() {
  }

  // ----------------------------------
  // セーブ/ロードの後処理
  // ----------------------------------
  function post_process() {
  }

  // ----------------------------------
  // セーブデータ用？ エラーダイアログ
  // ----------------------------------
  function savedata_error(noticeKey) {
    ::g_wipe.close(false, 0, 0x0000);

    result_dialog(noticeKey);
  }

  // ----------------------------------
  // 単体セーブ実行のラッパー
  // (auto指定の有無でsinglesaveかautosaveを行う)
  // ----------------------------------
  function save_single(_segment, _auto = true) {
    if ( !this.is_storage_check_ok(_segment, _auto, true) )
      return false;

    if (_auto)
      _segment.autosave();
    else 
      _segment.singlesave();

    return true;
  }

  // ----------------------------------
  // 単体ロード実行のラッパー
  // (auto指定の有無でsingleloadかautoloadを行う)
  // ----------------------------------
  function load_single(_segment, _auto = true)
  {
    if ( !this.is_storage_check_ok(_segment, _auto, false) )
      return false;

    if (_auto) 
      _segment.autoload();
    else 
      _segment.singleload();

    return true;
  }

  // ----------------------------------
  // 容量不足時のデリート（確認ダイアログあり）
  // 戻り値 : 確認ダイアログをキャンセル時にfalse
  // （容量不足によるセーブ失敗は各セーブ処理で検出してね）
  // ----------------------------------
  function delete_because_of_nospace(seg_type, auto = false, check = true) {
    print("start delete_at_nospace() \n");

    ::g_wipe.close(false, 0, 0x0000);
    local segment = get_backup_segment(seg_type);
    if ( ! ("listalldelete" in backup_manager_title) ) {
      // XXX 自分のタイトル以外のセーブデータを消す機能がない場合、警告を出してストレージを選び直せば成功したことにする
      ::result_dialog("NoticeMsg__CONFIRM_NOSPACE_STORAGE");
      if (auto)
        ::result_dialog("NoticeMsg__CONFIRM_SAVE_FAILED");

      return this.is_free_space_ok([seg_type]); // 空き容量チェックして足りなければデバイス選択
    }
    else {
      local ret = false;

      local code0_string = "";
      // セーブデータの必要サイズ
      if (check == true) {
        segment.datasize();
        while (segment.running)
          wait(0);
      }
      code0_string = segment.datasizeResult.sizeStr.slice(0);

      local msgKey = "NoticeMsg__CONFIRM_DELETE_SAVEDATA_BECAUSE_OF_NOSPACE";
      if (confirm_dialog(msgKey, code0_string, false)) {
        ::g_wipe.open(false, FRAME_SAVELOAD_WIPE, 0x00c0);

        backup_manager_title.listalldelete();
        while (backup_manager_title.running) 
          wait(0);

        // 成否不明だが、いちおうデリートを呼び出したので
        ret = true;
      }

      if (auto) 
        ::g_wipe.close(false, 0, 0x0000);

      return ret;
    }
  }

  // 破壊データ検出時のデリート（確認ダイアログあり）
  function delete_because_of_broken(seg_type, auto = false, check = true) {
    local ret = false;

    ::g_wipe.close(false, 0, 0x0000);

    if (confirm_dialog("NoticeMsg__CONFIRM_DELETE_SAVEDATA_BECAUSE_OF_BROKEN")) {
      ::g_wipe.open(false, FRAME_SAVELOAD_WIPE, 0x00c0);

      local backup_control = get_backup_control(seg_type);
      local segment = backup_control.get_backup_segment(seg_type);
      segment.fileId = segment.fileIdLastOperate; // 直前のセーブで選択して破壊判定されたものを選択
      segment.singledelete();
      while (segment.running) 
        wait(0);

      if (segment.success) 
        ret = true;
    }

    if (auto) 
      ::g_wipe.close(false, 0, 0x0000);
    printf("delete broken ret=%s\n", ret);

    return ret;
  }

  // ----------------------------------
  // 全セーブデータ対象デリート
  // ----------------------------------
  function delete_allsavedata() {
    this.pre_process(); // 前処理

    print("start delete_allsavedata() \n");

    ::g_wipe.open(false, FRAME_SAVELOAD_WIPE, 0x00c0);

    backup_manager_title.listalldelete();
    while (backup_manager_title.running) 
      wait(0);
		
    ::g_wipe.close(false, 0, 0x0000);

    this.post_process(); // 後処理
  }

  // ----------------------------------
  // capture 生成
  // ----------------------------------
  function create_capture() {
    print("create_capture()\n");
    if (!capture)  {
      local bounds = System.getScreenBounds();
      capture = Capture(::util_get_game_screen_layerFolder(), bounds.width, bounds.height);
      capture.setOffset(bounds.width / 2, bounds.height / 2);	// right & bottom	// なんで？
      capture.priority = ::g_emu_task.priority + 1;	// emu画面
      capture.visible = false;
    }

    local timer = 0;
    while ( SystemEtc.IsNeedPausing() && !::session_wait_check() ) {
      timer = 30;
      wait(0);
    }
    while (--timer > 0) wait(0);

    // ・SQCapture/SQCapturedはSQLayerBaseを継承している。
    // ・キャプるタイミングはMCapture::OnDraw()実行時となる。
    // ・なのでvisibleプロパティがキャプ処理のON/OFFトリガとなる。
    // PS3の場合：
    // ・キャプ処理(framebufferをコピー)は他のレンダリング処理と同じく
    // 　非同期処理の可能性あり。
    // ・なので一部layerのみの画をキャプりたい場合は、そーいう画ズラを
    // 　作り出して最低1フレは画ズラをキープしておく必要がある。
    // ・まぁいちおうキャプる対象はレンダリング中のframebufferじゃなくて
    // 　表示中のframebufferではあるんだけど。
    if ( SystemEtc.isSupportMultiView() ) {
      g_layerFolderDrcGame.visible = false; // XXX CAFEのテレビ画面のみキャプチャを行うためのパッチ
    }
    capture.visible = true;		// キャプON
    wait(0);
    capture.visible = false;	// キャプOFF
    if ( SystemEtc.isSupportMultiView() ) {
      g_layerFolderDrcGame.visible = true; // XXX CAFEのテレビ画面のみキャプチャを行うためのパッチ
    }
  }

  // ----------------------------------
  // capture 解放
  // ----------------------------------
  function release_capture() {
    print("release_capture()\n");
    capture = null;
  }

  function is_capture() {
    return (capture != null);
  }

  function get_capture() {
    return capture;
  }

  function _get_join_message(_key, _is_common = false) {
    local savedata_item_str = this._get_savedata_item_string(_key);
    if (savedata_item_str)
      return join_message_array(savedata_item_str,
                                codeList, 
                                _is_common ? current_series_name : current_title_name
                                );
    else
      return "";
  }

  function get_join_message_title(_key) {
    local savedata_item_str = this._get_savedata_item_string(_key);
    local dev_id = ::get_current_title_dev_id();
    local current_title = ::get_title_item(dev_id);
    local title_name = get_item_string( current_title["name"] ); // 現在のタイトル名
    local specTag = System.getSpec();
    if ( (specTag in current_title) && ("name" in current_title[specTag]) )
      title_name = get_item_string( current_title[specTag]["name"] )

    if (savedata_item_str)
      return join_message_array(savedata_item_str, codeList, title_name);
    else
      return null;
  }

  function set_is_disable_full_operation(_disable) {
    m_disable_full_operation = _disable; 
  }
  function get_is_disable_full_operation() {
    return m_disable_full_operation;
  }

  // ----------------------------------
  // 以下spec別override用
  // ----------------------------------
  function init_backup_manager(_is_common, _title) {
    return BackupManager( ::util_get_backup_control_layerFolder() );
  }

  function login_specdepend(_start_pad_id, _skip_autoload) {
  }

  function reset_user_specdepend(_start_pad_id, _skip_autoload) {
  }

  function get_save_data_string_path() {
    return "system/config/savedata_string.psb";
  }

  function set_segment_info_common_system(_segment) {
    _segment.comment = this._get_join_message("system_savedata_comment", true);
  }

  function set_segment_info_playlist(_segment, _is_common) {
    _segment.comment = this._get_join_message("sound_play_list_comment", _is_common);
  }

  function set_segment_info_title_system(_segment) {
    _segment.comment = this._get_join_message("title_savedata_comment");
  }

  function set_segment_info_emu_state(_segment, _title_no = 0, _on_init = false) {
    _segment.comment = this._get_join_message("statedata_comment");
  }

  function set_segment_info_emu_replay(_segment, _title_no = 0, _on_init = false) {
    _segment.comment = this._get_join_message("replaydata_comment");
  }

  function set_segment_icon(_seg_type, _data = null) {
  }

  function reset_segment_icon(_seg_type) {
  }

  function is_autosave_dialog_put_by_system() { // オートセーブ時のダイアログ表示をシステム側で行うか
    return false;
  }

  function is_storage_check_ok(_segment, _auto, _is_save, _force_select = false, _skip_first_confirm = false) {
    return true;
  }

  function clear_storage_reselected(_segment) { // ストレージ選択後の上書き確認用フラグをクリアする
  }

  function is_free_space_ok(_segment_lists) {
    return true;
  }

  function get_autosave_wait_dialog_warranty_time() {
    // オートセーブ/ロード待ちダイアログ表示期間の最低保証 (呼び元で参考にする)
    return 60;//(1*60);	// frame
  }

};


// -----------------------------------------------------------------------
// ラッパー関数
// -----------------------------------------------------------------------

// （常駐用）
g_backup_control <- null;

// 初期化
function setup_backup_control()
{
  if (::g_backup_control) 
    ::g_backup_control = null;

  ::g_backup_control = BackupControl();
  ::g_backup_control.init();

  if (! ::g_backup_control.is_init()) 
    printf("g_backup_control : initialize failed ...\n");
  else
    printf("g_backup_control : initialize successed.\n");
}

// BackupControlのインスタンスを取得する
// （必ずこの関数を使ってインスタンスを取得してください）
function get_backup_control( ... )
{
  return ::g_backup_control;
}

function login_backup_specdepend(_start_pad_id, _skip_autoload)
{
  if (::g_backup_control) {
    ::g_backup_control.login_specdepend(_start_pad_id, _skip_autoload);
  }
}

function reset_user_backup_specdepend()
{
  if (::g_backup_control) {
    ::g_backup_control.reset_user_specdepend();
  }
}

// ----
// セーブ（ステートデータ用）

// リストセーブを操作するpad_idを設定する
function set_list_control_pad_id(_pad_id)
{
  if ( ::get_backup_is_disabled() || null == _pad_id ) 
    return;

  local seg_types = [BackupSegmentTypes.EMU_STATE_L,
                     BackupSegmentTypes.EMU_STATE,
                     BackupSegmentTypes.EMU_REPLAY,
                     ];

  for (local no = 0; no < seg_types.len(); no++) {
    local segment = ::g_backup_control.get_backup_segment(seg_types[no]);
    if  ("setListControlInputIndex" in segment) {
      segment["setListControlInputIndex"](_pad_id);
    }
  }
}

// セーブ（ステートデータ用）
function save_state_data(_auto_access_file_id = null)
{
  local ret = false;
  if ( ::get_backup_is_disabled() ) 
    return ret;

  local backupStatedata = BackupStatedata(::g_backup_control);
  if (backupStatedata) {
    ret = backupStatedata.save_state_data(_auto_access_file_id);
  }

  return ret;
}

// エミュからステートデータ構造体に値を設定する
function set_state_data_from_emu()
{
  local ret = false;
  if ( ::get_backup_is_disabled() ) 
    return ret;

  local backupStatedata = BackupStatedata(::g_backup_control);
  if (backupStatedata) {
    ret = backupStatedata.set_state_data_from_emu();
  }

  return ret;
}


// ロード（ステートデータ用）
function load_state_data(_auto_access_file_id = null)
{
  local ret = false;
  if ( ::get_backup_is_disabled() ) 
    return ret;

  local backupStatedata = BackupStatedata(::g_backup_control);
  if (backupStatedata) {
    ret = backupStatedata.load_state_data(_auto_access_file_id);
  }

  return ret;
}

// デリート（ステートデータ用）
function delete_state_data(_auto_access_file_id = null)
{
  local ret = false;
  if ( ::get_backup_is_disabled() ) 
    return ret;

  local backupStatedata = BackupStatedata(::g_backup_control);
  if (backupStatedata) {
    ret = backupStatedata.delete_state_data(_auto_access_file_id);
  }

  return ret;
}

// セーブ（リプレイデータ用）
function save_replay_data(_no_set_header = false)
{
  local ret = false;
  if ( ::get_backup_is_disabled() ) 
    return ret;

  local backupReplaydata = BackupReplaydata(::g_backup_control);
  if (backupReplaydata) {
    ret = backupReplaydata.save_replay_data(_no_set_header);
  }

  return ret;
}

// リプレイデータの構造体に値を設定する
function set_replay_data_header(_mode = BackupGameMode.NORMAL, _submode = 0)
{
  local ret = false;

  local backupReplaydata = BackupReplaydata(::g_backup_control);
  if (backupReplaydata) {
    local replay_struct = ::g_backup_control.get_backup_struct(BackupSegmentTypes.EMU_REPLAY);
    ret = backupReplaydata.set_header(replay_struct, _mode, _submode);
  }

  return ret;
}

// リプレイデータをemuに設定する
function set_replay_data_to_emu(_replay_struct)
{
  local ret = false;

  local backupReplaydata = BackupReplaydata(::g_backup_control);
  if (backupReplaydata) {
    if ( backupReplaydata.check_header(_replay_struct) ) {
      ret = backupReplaydata.to_emu(_replay_struct);
    }
  }

  return ret;
}

// ロード（リプレイデータ用）
function load_replay_data(_replay_struct_safely = false)
{
  local ret = false;
  if ( ::get_backup_is_disabled() ) 
    return ret;

  local backupReplaydata = BackupReplaydata(::g_backup_control);
  if (backupReplaydata) {
    ret = backupReplaydata.load_replay_data(_replay_struct_safely);
  }

  return ret;
}

// デリート（リプレイデータ用）
function delete_replay_data()
{
  local ret = false;
  if ( ::get_backup_is_disabled() ) 
    return ret;

  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return ret;

  local backupReplaydata = BackupReplaydata(::g_backup_control);
  if (backupReplaydata) {
    ret = backupReplaydata.delete_replay_data();
  }

  return ret;
}

// backup_segment取得
function get_backup_segment(seg_type)
{
  local ret = null;

  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return ret;

  ret = ::g_backup_control.get_backup_segment(seg_type);

  return ret;
}

// backup_struct取得
function get_backup_struct(seg_type)
{
  local ret = null;

  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return ret;

  ret = ::g_backup_control.get_backup_struct(seg_type);

  return ret;
}

// セーブデータType毎のデータ個数を取得
function get_backup_segment_num(seg_type)
{
  local ret = null;

  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return ret;

  ret = ::g_backup_control.get_backup_segment_num(seg_type);

  return ret;
}

// セーブデータ構造体のフォーマットバージョン取得
function get_backup_struct_version(seg_type)
{
  local ret = null;

  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return ret;

  ret = ::g_backup_control.get_backup_struct_version(seg_type);

  return ret;
}

// 全セーブデータ対象デリート
function delete_allsavedata()
{
  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return;

  ::g_backup_control.delete_allsavedata();

}

// capture 生成
function create_savedata_capture()
{
  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return;

  ::g_backup_control.create_capture();

}

// capture 解放
function release_savedata_capture()
{
  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return;

  ::g_backup_control.release_capture();
}

// capture 確認
function is_savedata_capture()
{
  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return false;

  return ::g_backup_control.is_capture();
}

// capture 取得
function get_savedata_capture()
{
  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return null;

  return ::g_backup_control.capture;
}

// loginを促し、loginしているかを返す
function check_login(_start_pad_id)
{
  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return false;

  if ("login" in ::g_backup_control) {
    if ( SystemEtc.IsLogining() ) {
      return true;
    }
    else {
      return ::g_backup_control.login(_start_pad_id, true);
    }
  }
  else
    return true;
}

// ユーザ切り替えのためのPrimaryUser割り当てリセットを行う
function reset_user()
{
  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return false;

  if ("reset_user" in ::g_backup_control) {
    ::g_backup_control.reset_user();
  }
}

// backupを一切使わないかどうかを設定する(体験版やセーブなしプレイ用)
function set_backup_is_disabled(_disable)
{
  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return;

  ::g_backup_control.set_is_disable_full_operation(_disable);
}

// backupを一切使わないかどうかを取得する(体験版やセーブなしプレイ用)
function get_backup_is_disabled()
{
  if (!::g_backup_control || !::g_backup_control.is_init()) 
    return true;

  return ::g_backup_control.get_is_disable_full_operation();
}

// システムデータ用のbackup_structをクリアする
function clear_systemdata_backup_struct()
{
  local seg_type = BackupSegmentTypes.TITLE_DATA;
  local backup_control = get_backup_control(seg_type);
  local struct_title = backup_control.get_backup_struct(seg_type);
  struct_title.clear();
  if ( ::is_package_use_common_save_data() ) {
    local seg_type = BackupSegmentTypes.SYSTEM_DATA;
    local backup_control = get_backup_control(seg_type);
    local struct_common = backup_control.get_backup_struct(seg_type);
    struct_common.clear();
  }
}

// 保存するセーブデータ用の空きがあるかチェックする
function is_backup_free_space_ok(_segment_lists)
{
  if (::get_backup_is_disabled())
    return true;

  local backup_control = ::get_backup_control();
  return backup_control.is_free_space_ok(_segment_lists);
}
