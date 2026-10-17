//----------------------------------------------------------------------
// システムデータ管理クラス
//----------------------------------------------------------------------

//------------------------------
// データ ロード
//------------------------------

// （内部用）
function SystemData::_clear_load_result()
{
  m_load_result = 0; // オートロードのリザルトをクリア
}

function SystemData::_load_systemdata_common(_auto, _b_pause_menu) 
{
  // common_savedata

  local ret = false;
  if ( ::is_package_use_common_save_data() ) {
    local seg_type = BackupSegmentTypes.SYSTEM_DATA;
    local backup_control = get_backup_control(seg_type);
    if ( backup_control && backup_control.is_init() ) {
      printf("start load_systemdata_common(auto=%s)\n", _auto);
      local segment       = backup_control.get_backup_segment(seg_type);
      local struct_common = backup_control.get_backup_struct(seg_type);
      if (backup_control.load_single(segment, _auto)) {
        while (segment.running) 
          wait(0);

        if (segment.success) {
          if ( !::isDebugBuild() && ("getOwner" in segment) && !segment["getOwner"]() ) {
            if (!_auto) 
              ::g_wipe.close(false, 0, 0x0000);
            ::result_dialog("NoticeMsg__CONFIRM_UNJUST_SYSTEM_SAVE_DATA_BY_OTHER_PLAYER");
            struct_common.clear();
          }
          else {
            local struct_common_value = struct_common["root"]["data"];
            ret = this._copy_from_backup_struct_common(struct_common_value);
            if (ret) {
              if ( _auto || !segment.canceled ) {
                backup_control.clear_storage_reselected(segment); // ロードしたのでストレージ選択後の上書き確認用フラグをクリアする
              }
            }
            else {
              if (!_auto) 
                ::g_wipe.close(false, 0, 0x0000);
              ::result_dialog("NoticeMsg__SAVEDATA_VERSION_DIFFER_OR_BROKEN");
            }
          }
        }
        else {
          if (segment.broken) {
            if (!_auto) 
              ::g_wipe.close(false, 0, 0x0000);
            ::result_dialog("NoticeMsg__CONFIRM_BROKEN_SAVE_DATA");
          }
        }
      }
    }
  }
  else {
    ret = true;
  }

  return ret;
}

// （内部用）
function SystemData::_load_systemdata_title(_auto, _b_pause_menu) 
{
  // title_savedata

  local ret = false;
  local seg_type = BackupSegmentTypes.TITLE_DATA;
  local backup_control = get_backup_control(seg_type);
  if ( backup_control && backup_control.is_init() ) {
    printf("start load_systemdata_title(auto=%s)\n", _auto);
    local segment = backup_control.get_backup_segment(seg_type);
    if (backup_control.load_single(segment, _auto)) {
      while (segment.running) 
        wait(0);

      if (segment.success) {
        if ( !::isDebugBuild() && ("getOwner" in segment) && !segment["getOwner"]() ) {
          if (!_auto) 
            ::g_wipe.close(false, 0, 0x0000);
          ::result_dialog("NoticeMsg__CONFIRM_UNJUST_SYSTEM_SAVE_DATA_BY_OTHER_PLAYER");
          local struct_title = backup_control.get_backup_struct(seg_type);
          struct_title.clear();
        }
        else {
          local copy_result = this.copy_from_backup_struct(_auto, _b_pause_menu);
          if (SystemDataCopyFromStructResult.SUCCESS == copy_result) {
            if ( _auto || !segment.canceled ) {
              backup_control.clear_storage_reselected(segment); // ロードしたのでストレージ選択後の上書き確認用フラグをクリアする
            }
            ret = true;
          }
          else if (SystemDataCopyFromStructResult.DIFFER_VERSION == copy_result) {
            if (!_auto) 
              ::g_wipe.close(false, 0, 0x0000);
            ::result_dialog("NoticeMsg__SAVEDATA_VERSION_DIFFER_OR_BROKEN");
          }
        }
      }
      else {
        if (segment.broken) {
          if (!_auto) 
            ::g_wipe.close(false, 0, 0x0000);
          ::result_dialog("NoticeMsg__CONFIRM_BROKEN_SAVE_DATA");
        }
      }
    }
  }

  return ret;
}

function SystemData::_load_systemdata_title_affect_works(_struct_title_value)
{
  // save用structからゲーム設定をシステムに反映
  this.get_value(SystemDataValueIndex.SETTING_GAME).set_from_struct(_struct_title_value[SYSTEMDATA_KEY_SETTING_GAME]);
}

function SystemData::_load_systemdata_title_affect_sram(_struct_title_value, _auto, _b_pause_menu)
{
  local apply = true;
  local apply_to_emu = false;
  local found_valid_sram = this.get_value(SystemDataValueIndex.SRAM_DATA).load_from_struct(_struct_title_value[SYSTEMDATA_KEY_SRAM_DATA]);
  if (found_valid_sram && _b_pause_menu) { // pauseメニューから？
    if (!_auto) 
      ::g_wipe.close(false, 0, 0x0000);
    
    if (_auto && m_autosave_dialog)
      m_autosave_dialog.WaitEnd(); // autosave_waitダイアログclose
    apply = confirm_dialog("NoticeMsg__CONFIRM_EMU_SRAM_LOAD"); // 確認ダイアログ（SRAMイメージをemuに反映する？）
  }
  if (apply) {
    printf("load_systemdata_title_affect_sram() :Try to apply SRAM IMAGE\n");
    if (_b_pause_menu) { // pauseメニューから？
      if (_auto && m_autosave_dialog)
        m_autosave_dialog.WaitEnd(); // autosave_waitダイアログclose
      result_dialog("NoticeMsg__DONE_EMU_SRAM_LOAD");
    }
    apply_to_emu = this.get_value(SystemDataValueIndex.SRAM_DATA).update_system_current(::g_emu_task);
  }

  if (apply_to_emu) 
    request_reset_emulator();   // emulatorリセット要求
  else {
    printf("load_systemdata_title_affect_sram() : Apply SRAM IMAGE failed or canceled (apply=%s, apply_to_emu=%s...\n", apply, apply_to_emu);
    request_re_init_emulator(); // emulator再初期化リクエスト登録
  }
}

function SystemData::_copy_from_backup_struct_common(_struct_common_value)
{
  local ret = false;
  local seg_type = BackupSegmentTypes.SYSTEM_DATA;
  local backup_control = get_backup_control(seg_type);
  local struct_version = backup_control.get_backup_struct_version(seg_type);
  if (_struct_common_value._00_header._00_version != struct_version) { // versionチェック
    printf("_copy_from_backup_struct_common() : common : different version value=%08x / define=%08x\n", _struct_common_value._00_header._00_version, struct_version);
    ret = false;
  }
  else {
    this._correct_struct_all(_struct_common_value);
    this._copy_struct_all(this._get_data(current), _struct_common_value, SystemDataOpeBitFlag.COMMON); // dataStruct へコピー
    ret = true;
  }

  return ret;
}

// バックアップ構造体からシステムデータにコピーする
function SystemData::copy_from_backup_struct(_auto = false, _b_pause_menu = false)
{
  local ret = SystemDataCopyFromStructResult.UNINITIALIZED;
  local seg_type = BackupSegmentTypes.TITLE_DATA;
  local backup_control = get_backup_control(seg_type);
  local struct_value = backup_control.get_backup_struct(seg_type)["root"];
  local current_title_index = ::get_current_title_in_pack_index();
  local struct_title_value = struct_value["titles"][current_title_index];
  local struct_version = backup_control.get_backup_struct_version(seg_type);
  if ( !::is_package_use_common_save_data() && this.is_exist_common_filed_in_title_data() ) {
    // 共通セーブデータを使わないがまとめパックで共通領域がある場合、タイトルセーブデータの共通領域からコピー
    local struct_version = backup_control.get_backup_struct_version(BackupSegmentTypes.SYSTEM_DATA);
    local struct_common_value = struct_value["common"];
    if (struct_common_value._00_header._00_version == struct_version) {  // versionチェック
      this._copy_from_backup_struct_common(struct_common_value); // backup_structの共通領域からコピー
    }
    else {
      printf("load_systemdata_title()-common : different version value=%08x / define=%08x\n", struct_common_value._00_header._00_version, struct_version);
    }
  }
  if (struct_title_value._00_header._00_version != struct_version) {  // versionチェック
    printf("load_systemdata_title() : different version value=%08x / define=%08x\n", struct_title_value._00_header._00_version, struct_version);
    if (0 != struct_title_value._00_header._00_version)
      ret = SystemDataCopyFromStructResult.DIFFER_VERSION;
  }
  else {
    this._correct_struct_all(struct_title_value);
    this._copy_struct_all(this._get_data(current), struct_title_value, SystemDataOpeBitFlag.TITLE); // dataStruct へコピー

    this._load_systemdata_title_affect_works(struct_title_value);
    if (::util_is_sram_support()) 
      this._load_systemdata_title_affect_sram(struct_title_value, _auto, _b_pause_menu);

    ret = SystemDataCopyFromStructResult.SUCCESS;
  }

  return ret;
}

//------------------------------
// データ ロード （本体）
//------------------------------
function SystemData::load_systemdata(_auto, _b_pause_menu=false) 
{
  local ret = 0;

  local backup_control = get_backup_control( BackupSegmentTypes.SYSTEM_DATA );

  // セーブ/ロード 前処理
  if (backup_control) 
    backup_control.pre_process();

  printf("start load_systemdata(%s)\n", _auto);

  if (!_auto) 
    ::g_wipe.open(false, FRAME_SAVELOAD_WIPE, 0x00c0);

  if ( this._load_systemdata_common(_auto, _b_pause_menu) )
    ret = ret | SystemDataOpeBitFlag.COMMON;

  wait(0);

  if ( this._load_systemdata_title(_auto, _b_pause_menu) )
    ret = ret | SystemDataOpeBitFlag.TITLE;

  if (ret) {
    this._copy_data_all(current^1, current); // 巻き戻し用バックアップに退避

    if ( ::get_config_use_purchase_record() ) {
      // 購入履歴比較
      if ( !this.get_data_purchase_record_myself() ) {
        ret = ret & ~SystemDataOpeBitFlag.COMMON; // 初回起動ならシステムもセーブ
      }
      this.set_data_purchase_record_myself(); // 自身の購入履歴をset
    }
    this.update_system_all();               // 現在の設定をシステムに反映
    m_autosave_req = 0;                     // AUTOセーブ要求をクリア
  }
  else  {
    // ココではエラーダイアログは出さない。必要なら呼び元で出すこと。
  }

  if (_auto) {
    m_load_result = ret;
    printf("load result %x\n", m_load_result);
  }

  if (!_auto) 
    ::g_wipe.close(false, 0, 0x0000);

  // セーブ/ロード 後処理
  if (backup_control) 
    backup_control.post_process();

  return ret;
}

//------------------------------
// データ セーブ
//------------------------------

// （内部用）
function SystemData::_save_systemdata_common_body(_auto) 
{
  // system_savedata
  local ret            = false;
  local seg_type = BackupSegmentTypes.SYSTEM_DATA;
  local backup_control = get_backup_control(seg_type);
  if (backup_control && backup_control.is_init()) {
    backup_control.swap_backup_title(seg_type);  // title文字列をスリかえる

    local segment             = backup_control.get_backup_segment(seg_type);
    local struct_common_value = backup_control.get_backup_struct(seg_type)["root"]["data"];

    struct_common_value._00_header._00_version = backup_control.get_backup_struct_version(seg_type); // version埋め込み
    this._copy_struct_all(struct_common_value, this._get_data(current), SystemDataOpeBitFlag.COMMON); // backup_struct へコピー

    do {
      if (!backup_control.save_single(segment, _auto))
        break;

      while (segment.running) 
        wait(0);

      if (segment.success) {
        // （autoじゃない場合は成功か途中キャンセルか不明）
        printf("save_systemdata(%s) : system : success !!\n", _auto.tostring());
        if ( _auto || !segment.canceled ) {
          backup_control.clear_storage_reselected(segment); // セーブしたのでストレージ選択後の上書き確認用フラグをクリアする
        }
        ret = true;
        break;
      }

      if (_auto && m_autosave_dialog) {
        m_autosave_dialog.WaitEnd();
      }

      // 容量不足によるセーブ失敗？
      if (!segment.nospace) 
        break;

      // 容量不足時のデリート（確認ダイアログあり）
      if (!backup_control.delete_because_of_nospace(seg_type, _auto))
        break;

      if (_auto && m_autosave_dialog) {
        m_autosave_dialog.Start();
      }

    } while(1);

    // title文字列を元に戻す
    backup_control.swap_backup_title(seg_type, true);
  }

  return ret;
}

// （内部用）
function SystemData::_save_systemdata_title_body(_auto, _from_pause_menu) 
{
  // title_savedata
  local ret            = false;
  local seg_type       = BackupSegmentTypes.TITLE_DATA;
  local backup_control = get_backup_control(seg_type);
  if ( backup_control && backup_control.is_init() ) {
    backup_control.swap_backup_title(seg_type); // title文字列をスリかえる

    ::chronos_capture_sram();
    this.copy_to_backup_struct();

    local segment = backup_control.get_backup_segment(seg_type);

    do {
      if (!backup_control.save_single(segment, _auto))
        break;

      while (segment.running) 
        wait(0);

      if (segment.success) {
        // （autoじゃない場合は成功か途中キャンセルか不明）
        printf("save_systemdata(%s) : title : success !!\n", _auto.tostring());
        if ( _auto || !segment.canceled ) {
          backup_control.clear_storage_reselected(segment); // セーブしたのでストレージ選択後の上書き確認用フラグをクリアする
        }
        ret = true;
        break;
      }

      if (_auto && m_autosave_dialog) {
        m_autosave_dialog.WaitEnd();
      }

      // 容量不足によるセーブ失敗？
      if (!segment.nospace) 
        break;

      // 容量不足時のデリート（確認ダイアログあり）
      if (!backup_control.delete_because_of_nospace(seg_type, _auto))
        break;

      if (_auto && m_autosave_dialog) {
        m_autosave_dialog.Start();
      }

    } while (1);

    backup_control.swap_backup_title(seg_type, true); // title文字列を元に戻す

    if ( (!ret) && ::util_is_sram_support() && _from_pause_menu)
      this.RequestAutosave(SystemDataOpeBitFlag.TITLE); // ポーズメニューからの手動セーブに失敗した時は次にオートセーブが必ず発動するようにする
  }

  // Persist native autosaves into the current game immediately, including
  // shutdown and pause saves. No SRAM ownership change occurs here.
  if (ret && ::chronos_save_enabled() && ::s_chronos_active_game_id != null)
    ::chronos_flush_sram();
  return ret;
}

function SystemData::_save_systemdata_title_get_works(_struct_title_value)
{
  // ゲーム設定をsave用structにコピー
  this.get_value(SystemDataValueIndex.SETTING_GAME).get_to_struct(_struct_title_value[SYSTEMDATA_KEY_SETTING_GAME]);
}

function SystemData::_save_systemdata_title_get_sram(_struct_title_value)
{
  // SRAMをsystemdataからsave用structにコピー
  this.get_value(SystemDataValueIndex.SRAM_DATA).save_to_struct(_struct_title_value[SYSTEMDATA_KEY_SRAM_DATA]);
}

function SystemData::_save_systemdata_common(_auto, _accessFlag, _ret)
{
  if ( (_accessFlag & SystemDataOpeBitFlag.COMMON) && !(m_load_result & SystemDataOpeBitFlag.COMMON) && ::is_package_use_common_save_data() ) {
    printf("start save_systemdata_common_body(auto=%s)\n", _auto);
    local res = this._save_systemdata_common_body(_auto);
    if (res) 
      _ret = _ret | SystemDataOpeBitFlag.COMMON;

    if (_auto && !res) {
      // オートセーブ失敗時
      ::g_wipe.close(false, 0, 0x0000);
    }
    else if (_auto && m_autosave_dialog) {
      m_autosave_dialog.Start();
    }
    wait(0);
  }
  else {
    _ret = _ret | SystemDataOpeBitFlag.COMMON;
  }

  return _ret;
}

function SystemData::_save_systemdata_title(_auto, _from_pause_menu, _accessFlag, _ret)
{
  if ( (_accessFlag & SystemDataOpeBitFlag.TITLE) && !(m_load_result & SystemDataOpeBitFlag.TITLE) ) {
    printf("start save_systemdata_title_body(auto=%s)\n", _auto);
    local res = this._save_systemdata_title_body(_auto, _from_pause_menu);
    if (res) 
      _ret = _ret | SystemDataOpeBitFlag.TITLE;

    if (_auto && !res) {
      // オートセーブ失敗時
      ::g_wipe.close(false, 0, 0x0000);
    }
    else {
      // ココではエラーダイアログは出さない。必要なら呼び元で出すこと。
    }
  }
  else {
    _ret = _ret | SystemDataOpeBitFlag.TITLE;
  }

  return _ret;
}

// 現在のシステムデータ内容をバックアップ構造体の該当タイトル部分にコピーする
function SystemData::copy_to_backup_struct()
{
  local seg_type = BackupSegmentTypes.TITLE_DATA;
  local backup_control = get_backup_control(seg_type);
  local current_title_index = ::get_current_title_in_pack_index();
  local struct_value = backup_control.get_backup_struct(seg_type)["root"];
  local struct_title_value = struct_value["titles"][current_title_index];
  struct_title_value._00_header._00_version = backup_control.get_backup_struct_version(seg_type);  // version埋め込み
  this._copy_struct_all(struct_title_value, this._get_data(current), SystemDataOpeBitFlag.TITLE);  // backup_struct へコピー

  if ( !::is_package_use_common_save_data() && this.is_exist_common_filed_in_title_data() ) {
    // 共通セーブデータを使わないがまとめパックで共通領域がある場合、タイトルセーブデータの共通領域へコピー
    local struct_common_value = struct_value["common"];
    struct_common_value._00_header._00_version = backup_control.get_backup_struct_version(BackupSegmentTypes.SYSTEM_DATA);  // version埋め込み
    this._copy_struct_all(struct_common_value, this._get_data(current), SystemDataOpeBitFlag.COMMON); // backup_structの共通領域へコピー
  }

  if (::util_is_sram_support()) {
    this._save_systemdata_title_get_sram(struct_title_value);
  }
  this._save_systemdata_title_get_works(struct_title_value);
}

// 現在のタイトルのBackup用StructValueを取得する
function SystemData::get_current_backup_struct_title_value()
{
  local seg_type = BackupSegmentTypes.TITLE_DATA;
  local backup_control = get_backup_control(seg_type);
  local current_title_index = ::get_current_title_in_pack_index();
  local struct_value = backup_control.get_backup_struct(seg_type)["root"];
  local struct_title_value = struct_value["titles"][current_title_index];

  return struct_title_value;
}

//------------------------------
// データ セーブ （本体）
//------------------------------
function SystemData::save_systemdata(_auto, _from_pause_menu = false, _accessFlag = null) 
{
  local ret = 0;
  local backup_control = get_backup_control( BackupSegmentTypes.SYSTEM_DATA );
  
  if ( null == _accessFlag )
    _accessFlag = SystemDataOpeBitFlag.BOTH;

  if (backup_control) 
    backup_control.pre_process(); // セーブ/ロード 前処理

  if ( !_auto ) 
    ::g_wipe.open(false, FRAME_SAVELOAD_WIPE, 0x00c0);

  ret = this._save_systemdata_common(_auto, _accessFlag, ret);

  if (!_auto) 
    ::g_wipe.open(false, FRAME_SAVELOAD_WIPE, 0x00c0); // 空き容量不足時のリトライをキャンセルして暗転が解除されているかも？

  ret = this._save_systemdata_title(_auto, _from_pause_menu, _accessFlag, ret);

  if (!_auto) 
    ::g_wipe.close(false, 0, 0x0000);

  if (ret) {
    this._copy_data_all(current^1, current); // 巻き戻し用バックアップに退避
    m_autosave_req = m_autosave_req & ~ret;  // AUTOセーブ要求をクリア
  }

  if (backup_control) 
    backup_control.post_process(); // セーブ/ロード 後処理

  return ret;
}

//------------------------------
// AUTOロードを行う
//------------------------------
function SystemData::ExecAutoload() 
{
  if ( ::get_backup_is_disabled() )
    return 0;
  else {
    local ret = this.load_systemdata(true); // autoロード
    printf("ExecAutoload: ret = %x\n", ret);

    return ret;
  }
}

//------------------------------
// AUTOセーブ要求
//------------------------------
function SystemData::RequestAutosave(_flag = null) 
{
  if ( !::is_package_use_common_save_data() ) {
    _flag = null; // 共通セーブデータを使わない場合、常に両方セーブ要求して必ずタイトル側がセーブされるようにする
  }

  local request_flag = (null == _flag) ? SystemDataOpeBitFlag.BOTH : _flag;
  m_autosave_req = m_autosave_req | request_flag;
}

function SystemData::IsRequestedAutosave() 
{
  return m_autosave_req;
}

//------------------------------
// 指定した設定が変更されているか調べ、変更があればシステムに反映後オートセーブリクエスト
//------------------------------
function SystemData::CheckValueChangeAutosave(_value_index = null)
{
  if ( null == _value_index ) {
    local res = this.is_change_data_all();
    if (res) {
      this.complete_all(true);
      this.RequestAutosave(res);  // AUTOセーブ要求
	}
  }
  else {
    local value = this.get_value(_value_index);
    local res = value.is_change();
    if (res) {
      value.complete(true);
      this.RequestAutosave(res); // AUTOセーブ要求
    }
  }
}

//------------------------------
// AUTOセーブを試みる
//------------------------------
function SystemData::TryAutosave(bForced = false, bWhenSramSave = false) 
{
	::g_frameCount.setSaveExec(1);	//セーブ中


  local ret = true;

  local res_wait_check = ::g_CommonSession.wait_check();
  if (res_wait_check != SessionWaitCheckResult.LOGOUT_RESET
   && res_wait_check != SessionWaitCheckResult.CHANGE_USER_REQ_RESET
   && !::get_backup_is_disabled()
  ) {
    if ( bForced )
      m_autosave_req = SystemDataOpeBitFlag.BOTH;

    if ( m_autosave_req && this.get_data_setting_savedata__autosave() ) {

      if (bWhenSramSave)
        this.capture_system_all(); // ノーマルゲーム中からのSRAMオートセーブ呼出時はsystemdataに取り込む

      local msgKey = bWhenSramSave ? "NoticeMsg__AUTO_SAVING_SYTEMDATA_SRAM" : "NoticeMsg__AUTO_SAVING_SYTEMDATA";
      local errKey = [
                      "NoticeMsg__AUTO_SAVING_SYSTEMDATA_FAILED",
                      ];
      while (1) {
        // autosave_waitダイアログをthreadで表示
        local backup_control = get_backup_control( BackupSegmentTypes.TITLE_DATA );
//        m_autosave_dialog = backup_control.is_autosave_dialog_put_by_system() ? null : AutosaveWaitDialog(msgKey);
        m_autosave_dialog = null;
        if (m_autosave_dialog) {
          m_autosave_dialog.Start(true);
        }

        local frameCount = FrameCount();
        ret = this.save_systemdata(true, false, m_autosave_req);                  // systemdata autoセーブ
/*
        local remainCount = backup_control.get_autosave_wait_dialog_warranty_time() - frameCount.count; // ダイアログ表示期間の最低保証
        printf("remainCount = %d\n", remainCount);
        if (remainCount > 0) 
          wait(remainCount-1);
*/
        // autosave_waitダイアログclose
        if (m_autosave_dialog) {
          m_autosave_dialog.WaitEnd();
        }
        m_autosave_dialog = null;

        if (!ret) {
          result_dialog(errKey);
          // (エラーによる) AUTOSAVE : OFF 
          this._autosave_error_off();
        }
        break;
      }
    }
  }

  m_autosave_req = 0; // AUTOセーブ要求をクリア

	//初期値以外の場合はファイルを作成する
	local ch = false;
	if ( ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).isInitDataChange( ) ) //
	{
		printf("save data BACKUP_FLAGS change \n");
		ch = true;
	}
	if ( ::g_systemdata.get_value(SystemDataValueIndex.SRAM_DATA).isInitDataChange( ) ) //
	{
		printf("save data SRAM_DATA change \n");
		ch = true;
	}
	if( ch == true )
	{
		//変更されている
		printf("save data change start\n");
		::g_frameCount.initfileSave();	//初期化確認用ファイル作成
		printf("save data change end\n");
	}
	else
	{
		//変更されていない
		printf("save data init  start\n");
		::g_frameCount.initfileDelete();	//初期化確認用ファイル削除
		printf("save data init  end\n");
	}

	::g_frameCount.setSaveExec(0);	//セーブ中

  return ret;
}

//------------------------------
// SRAMのAUTOセーブ(主にtitle_standaloneのscrpitから呼ばれる)
//------------------------------
function SystemData::AutosaveSram()
{
  ::g_emu_task.pause = true;
  this.RequestAutosave(SystemDataOpeBitFlag.TITLE);
  this.TryAutosave(false, true); // AUTOセーブを試みる（これまでのAUTOセーブ要求はクリアされる）
  ::g_emu_task.pause = false;
}
