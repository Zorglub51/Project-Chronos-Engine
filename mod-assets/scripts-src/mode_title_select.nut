//----------------------------------------------------------------------
// タイトル選択画面
//----------------------------------------------------------------------

//デモバーションフラグはconst.nutへ移動しました。
//const DEMOVERSISON = 0;	//デモバージョンの場合は1


//ソートタイプ
const SORTTYPE_NAME = 0;
const SORTTYPE_DATE = 1;
const SORTTYPE_GENRE = 2;
const SORTTYPE_PLNUM = 3;


//プライオリティ
const PRI_BG_BACK = 0;
const PRI_NECKUN = 1;
const PRI_BG = 2;
const PRI_PKG = 5;
const PRI_CSL = 6;
const PRI_TITLEBAR = 7;
const PRI_TITLEBAR_NAME = 8;
const PRI_BG_DOWN = 9;
const PRI_TEXT = 10;
const PRI_MASK = 11;
const PRI_DEBUG = 12;

//デモを有効にする場合は1にする
const DEMO_USE = 1;

//ビルド日時を表示する場合は1
const BUILDDATE_DIP = 0;

//タイトル表示を飛ばす
const TITLESKIP = 1;

//ゲーム起動演出
const LINENUP_WAIT = 60;

//タイトル数
s_titleNum <- null;

// FOLDER HACK: per-lineup cursor memory.
// Indexed by LINEUP_JP / LINEUP_US (0 / 1). null means "no saved cursor for
// this lineup yet — fall back to s_last_index". Updated on every toggle so
// jp→us→jp returns to the original jp cursor. Tracks the ROOT-pack cursor
// only — saving is skipped while inside a folder so the folder's cursor
// doesn't leak into the lineup-toggle-back path.
s_last_index_per_linenp <- [null, null];

// FOLDER HACK: are we currently inside a folder pack? Set true on csize==10
// (enter), false on csize==11 (exit).
s_in_folder <- false;

// Inspect the destination before changing mounts, saves or the live menu.
// An empty pack has 200 DUMMY slots but no carousel entries; constructing
// MenuModeSelectPkg for it indexes an empty array and leaves a black screen.
function can_enter_lineup(lineup)
{
	local name = (lineup == LINEUP_JP) ? "jp" : "us";
	// Resource paths are relative to /usr/game, even with a leading slash.
	local path = "../../mnt/usb/library/published/folders/" + name + "/_root/title_mode_top.psb";
	local rsc = Resource();
	local count = 0;
	try {
		rsc.load(path);
		while (rsc.loading) wait(0);
		local config = rsc.find(path).root;
		count = config[(lineup == LINEUP_JP) ? "titleNum" : "titleNumTG"];
	} catch (e) {
		printf("[FH-LINEUP] cannot read %s: %s\n", path, e.tostring());
	}
	rsc.unload();
	printf("[FH-LINEUP] destination %s has %d entries\n", name, count);
	return count > 0;
}

//NEC君の数
const NECKUN_NUM = 11;

//吹き出しアニメーション
const FUKIDASHI_ANIME = 5.0;

const KEYWAIT_PKG = 12;
// LINEUPMAX moved to const.nut so mode_demo.nut can see it

function getMotionPath()
{
	//本体リージョンに合わせて読み込むモーションを変える
	local devid = ::get_package_dev_id();
	local path = null;
	switch( devid )
	{
	case DEVID_JP:
		if( s_last_linenp == LINEUP_JP )
		{
			path = conv_path(MENU_JP_TITLESELECT_JP_MOTION_PATH, devid);
		}
		else
		{
			path = conv_path(MENU_JP_TITLESELECT_US_MOTION_PATH, devid);
		}
		break;
	case DEVID_US:
	case DEVID_EU:
	case DEVID_AS:
		if( s_last_linenp == LINEUP_JP )
		{
			path = conv_path(MENU_US_TITLESELECT_JP_MOTION_PATH, devid);
		}
		else
		{
			path = conv_path(MENU_US_TITLESELECT_US_MOTION_PATH, devid);
		}
		break;
	}

	printf("getMotionPath() = %d path = %s\n", s_last_linenp, path);
	return path;
}

//選択されたロムテーブルを取得する
function getlastIndex( index = null )
{
	if( index == null )
	{
		return s_indextable[s_last_index];
	}
	else
	{
		return s_indextable[index];
	}
}

function getLanguageIndexOffset( )
{
	local indexoffset = 0;
	local langspan = LINEUPMAX * 2;
	switch( getlastLanguage() )
	{
	case LANGUAGE_JPN:
		indexoffset = langspan * 0;
		break;
	default:
		indexoffset = langspan * 1;	//日本語以外は英語テーブルを参照する
		break;
	}
/*
	switch( getlastLanguage() )
	{
	case LANGUAGE_JPN:
		indexoffset = langspan * 0;
		break;
	case LANGUAGE_ENG:
		indexoffset = langspan * 1;
		break;
	case LANGUAGE_FRA:
		indexoffset = langspan * 2;
		break;
	case LANGUAGE_ITA:
		indexoffset = langspan * 3;
		break;
	case LANGUAGE_GER:
		indexoffset = langspan * 4;
		break;
	case LANGUAGE_SPA:
		indexoffset = langspan * 5;
		break;
	}
*/	
	if( s_last_linenp != LINEUP_JP )
	{
		indexoffset = indexoffset + LINEUPMAX;
	}

	return ( indexoffset );
}

function getLinenpIndexOffset( )
{
	local indexoffset = 0;
	if( s_last_linenp != LINEUP_JP )
	{
		indexoffset = LINEUPMAX;
	}

	return ( indexoffset );
}


//ソート方法を指定してパッケージのインデックスを取得する
function getPkgIndex( id, sortType, config )
{
	local ofs = getLanguageIndexOffset( );

	local rc = 0;
	switch( sortType )
	{
	case SORTTYPE_NAME:
		//名前順
		rc = config["items"][id + ofs]["sor_name"];
		break;
	case SORTTYPE_DATE:
		//日付順
		rc = config["items"][id + ofs]["sor_date"];
		break;
	case SORTTYPE_GENRE:
		//ジャンル
		rc = config["items"][id + ofs]["sor_genr"];
		break;
	case SORTTYPE_PLNUM:
		//プレイ人数
		rc = config["items"][id + ofs]["sor_pnum"];
		break;
	}
	if( DEMOVERSISON == 1 )
	{
		//デモバージョン
		rc = config["items"][id + ofs]["sor_demo"];
	}

	return ( rc );
}

function getLinenpOffsetConfigData( idx, tag )
{
	local indexoffset = getLinenpIndexOffset( );	//起動するリージョンによってオフセットを加える
	local sortindex = s_indextable[idx] + indexoffset;

	local val = m_config["items"][sortindex][tag];
	
	return( val );
}
function mode_title_select(_menu_motion = null, _start_pad_id = null)
{
::g_frameCount.logFileWrite("");
::g_frameCount.logFileWrite("app start ==========================");
::g_frameCount.logFileWrite("");

	::g_frameCount.sqgc();

  local path = ::conv_checked_path( SCRIPT_TITLE_MODE_SELECT_PATH, ::get_package_dev_id() );
  if ( null != path ) {
    ::util_load_script(path); // タイトルスクリプト内でMenuMoteTitleSelectを置き換える運用を想定
  }

	if( s_indextable == null )
	{
		s_indextable = [];
		local i = 0;
		for ( i = 0; i < LINEUPMAX + 10 ; i++ )
		{
			s_indextable.append(LINEUPMAX);
		}
	}

	if( DEMOVERSISON == 1 )
	{
		//デモバージョンではセーブデータは初期化される。
		::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).init( ); //初期化
		::g_systemdata.get_value(SystemDataValueIndex.SRAM_DATA).init( ); //初期化
	}
	if ( ::g_bg_count == null )
	{
		::g_bg_count = 0;
	}

	if ( s_last_index == null )
	{
		s_last_index = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_08_last_selectTitle();
	}
	if ( s_last_sortType == null)
	{
		s_last_sortType = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_07_last_sortType();
	}
	if ( s_last_viweType == null )
	{
		s_last_viweType = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_09_last_viweType();
	}
	if ( s_last_pkgregion == null )
	{
		s_last_pkgregion = PKGREGION_JP;
	}
	if ( s_last_linenp == null )
	{
		s_last_linenp = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_75_hardType();
	}
	if ( s_last_language == null )
	{
		local devid = ::get_package_dev_id();
		switch( devid )
		{
		case DEVID_JP:
			s_last_language = LANGUAGE_JPN;
			break;
		case DEVID_US:
			s_last_language = LANGUAGE_ENG;
			break;
		}
	}
	
	if( s_menu_reboot == null )
	{
		s_menu_reboot = false;
	}
	if( s_linenp_reboot == null )
	{
		s_linenp_reboot = true;
	}

	if( g_fristExec == null )	//初回起動か？
	{
		g_fristExec = 0;
	}
	if( s_frame_offset == null )	//
	{
		s_frame_offset = 0;
		setFrameinOut( );
	}
	if( s_frame_offset_move == null )	//
	{
		s_frame_offset_move = 0;
	}

	if( s_ui_motionPath == null )
	{
		s_ui_motionPath = conv_path(MENU_UI_TITLESELECT_MOTION_PATH);
	}

	local devid = ::get_package_dev_id();
	printf("/// devid = %d ///\n", devid);


//	printf("get_70_firstSelectLang = %d\n", ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_70_firstSelectLang( ));

	if( DEMOVERSISON == 0 )
	{
/*
			local devid = ::get_package_dev_id();
			switch( devid )
			{
			case DEVID_JP:
//				::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_06_last_language(s_last_language);
				s_last_language = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_06_last_language();
				break;
			case DEVID_US:
			case DEVID_EU:
			case DEVID_AS:
				//初回起動時の言語設定
				if( ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_70_firstSelectLang( ) == 0 )
				{
					::util_load_script("system/script/mode_title_select_settinglang.nut");
					::mode_title_select_settinglang_main();
					s_last_language = ::getSettingLanguage();
					::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_06_last_language( s_last_language );
				}
				else
				{
					s_last_language = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_06_last_language();
				}
				::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_70_firstSelectLang(1);
				break;
			}
			::g_systemdata.TryAutosave(true);
*/
			//どの本体リージョンも必ずやる
			local devid = ::get_package_dev_id();
			//初回起動時の言語設定
			if( ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_70_firstSelectLang( ) == 0 )
			{
				::util_load_script("system/script/mode_title_select_settinglang.nut");
				::mode_title_select_settinglang_main();
				s_last_language = ::getSettingLanguage();
				::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_06_last_language( s_last_language );
			}
			else
			{
				s_last_language = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_06_last_language();
			}
			::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_70_firstSelectLang(1);
			::g_systemdata.TryAutosave(true);
	}
	::setLanguageTag( s_last_language );	//システム言語を設定する

	printf("get_70_firstSelectLang = %d\n", ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_70_firstSelectLang( ));
	printf("get_06_last_language   = %d\n", ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_06_last_language( ));
	printf("get_75_hardType        = %d\n", ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_75_hardType( ));

	if( TITLESKIP == 0 )
	{ 
		if( g_fristExec == 0 )
		{
			{
				//ワーニング表示
				::util_load_script("system/script/mode_title_select_warning.nut");
				::mode_title_select_warning_main();
			}
			{
				//コナミロゴ表示
				::util_load_script("system/script/mode_title_select_logo.nut");
				::mode_title_select_logo_main();
			}
			{
				//タイトル表示
				::util_load_script("system/script/mode_title_select_title.nut");
				::mode_title_select_title_main();
			}
		}
	}
	else
	{
		if( g_fristExec == 0 )
		{
			::init_system_emulator();
		}
	}
	g_fristExec = 1;	//初回起動フラグを立てる
	::init_wallpaperBG();
	::g_wallpaperBG.open();	//背景を残しておく


  local menu = MenuModeTitleSelect(_start_pad_id);

	if( s_last_linenp == LINEUP_JP )
	{
		//日本バージョン
	  if( ::g_menu_sound.get_bgm_id() != "bgm_menu_normal" ) 
	  {
	    ::g_menu_sound.setup_bgm( "bgm_menu_normal" );
	    while ( !::g_menu_sound.is_bgm_setuped() ) 
	      wait(0);

	    ::g_menu_sound.play_bgm();
	    ::g_menu_sound.set_bgm_volume(BGM_TITLE_VOLUME);
	  }
	  else
	  {
	    ::g_menu_sound.pause_bgm(false);
	    Sound.animateVoiceVolume("bgm_menu_normal", BGM_TITLE_VOLUME, FRAME_BGM_TITLE_FADE_OUT, 0);
	  }
	}
	else
	{
		//海外バージョン
	  if( ::g_menu_sound.get_bgm_id() != "bgm_menu_cdrom" ) 
	  {
	    ::g_menu_sound.setup_bgm( "bgm_menu_cdrom" );
	    while ( !::g_menu_sound.is_bgm_setuped() ) 
	      wait(0);

	    ::g_menu_sound.play_bgm();
	    ::g_menu_sound.set_bgm_volume(BGM_TITLE_VOLUME);
	  }
	  else
	  {
	    ::g_menu_sound.pause_bgm(false);
	    Sound.animateVoiceVolume("bgm_menu_cdrom", BGM_TITLE_VOLUME, FRAME_BGM_TITLE_FADE_OUT, 0);
	  }
	}

	menu.exec();

	return ;
}

// テスト用簡易テキストメニュータイトル選択
class MenuModeTitleSelectBase {

  m_start_pad_id = null;
  m_titles = null;
  m_menu = null;
  m_new_index = null;

  //ここを保存しておけばカーソル位置は再現されるはず
  m_current_index = null;
  m_current_sortType = null;
  m_current_viweType = null;
  m_current_pkgregion = null;
  m_current_language = null;
  m_current_linenp = null;
  //ここまで
  m_isSelectRun = null;
  m_selectCount = null;
  
  constructor(_start_pad_id) {
    m_start_pad_id = _start_pad_id;
    printf("MenuModeTitleSelectBase: start_pad_id = %s\n", m_start_pad_id);
    m_titles = null;
    m_menu = null;
		m_selectCount = 0;

		printf( "s_last_index = %d\n", s_last_index );

		m_current_index = s_last_index;
		m_current_sortType = s_last_sortType;
		m_current_viweType = s_last_viweType;
		m_current_pkgregion = s_last_pkgregion;
		m_current_language = s_last_language;
		m_current_linenp = s_last_linenp;
  }

  function exec() {
    local loop_result = false;
    this._init();
    local retry = false;
    do {
      local result = this.exec_body();

      if (result != null) {

				printf("[FH-EXEC] result=%d (begin launch path)\n", typeof result == "integer" ? result : -999);
				::delete_wallpaperBG();
				printf("[FH-EXEC] after delete_wallpaperBG\n");
				::init_wallpaper();
				printf("[FH-EXEC] after init_wallpaper\n");
				::g_wallpaper.setWallpaper();		//壁紙設定
				printf("[FH-EXEC] after setWallpaper\n");
				::g_wallpaper.open();
				printf("[FH-EXEC] after wallpaper.open\n");

				local screenStting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_05_last_screen( );	//セーブデータから取得する
				if( screenStting == SCREENSETTING_GT )
				{
					//GTの起動画面白
					::g_wipe.setColorWeight(0x808080ff);
					::g_wipe.setPriority(::g_emu_task.getPriority() + 1);
					::g_wipe.open(false, 0, 255);

//					::g_emu_task.SetGTScreen( true );
					::g_emu_task.SetGTScreen( false );	//GTも224にする
				}
				else 
				{
					::g_emu_task.SetGTScreen( false );
				}
				
				m_menu = null;							//クラスの参照を消しておく
				setFrameinOut( );
				printf("[FH-EXEC] after setFrameinOut\n");

        if( result == -1 )
        {
          loop_result = ::g_demo_control.exec(this);
        }
        else
        {
          m_new_index = result;
          printf("[FH-EXEC] before util_load_script(mode_main)\n");
          ::util_load_script( conv_path(SCRIPT_MODE_MAIN_PATH) );
          printf("[FH-EXEC] after util_load_script, before before_change_title\n");
          this.before_change_title();
          printf("[FH-EXEC] after before_change_title, before mode_main_change_title(%d)\n", m_new_index);
          ::mode_main_change_title(m_new_index);
          printf("[FH-EXEC] after mode_main_change_title, before after_change_title\n");
          this.after_change_title();
          printf("[FH-EXEC] after after_change_title, before mode_main\n");

//          ::g_menu_bg.open(true);
//          ::g_menu_sound.pause_bgm(true);
          loop_result = ::mode_main();
          printf("[FH-EXEC] mode_main returned: %s\n", loop_result.tostring());
//          ::g_menu_bg.close();
        }
        if (loop_result != SessionWaitCheckResult.ACCEPT_INVITE_RESET) {
          retry = true;
        }
      }
      else {
				m_menu = null;							//クラスの参照を消しておく
        // FOLDER HACK: lineup toggle / linenp reboot path
        if (::s_linenp_reboot == true) {
          printf("[FH-EXEC] linenp reboot detected: s_last_linenp=%d m_current_linenp=%d\n",
                 s_last_linenp, m_current_linenp);
          // Sync m_current_linenp from script-level (may have been updated
          // by the toggle in m_menu) — the new MenuModeTitleSelectSub takes
          // current_linenp as a constructor arg, so it must be correct.
          m_current_linenp = s_last_linenp;
          printf("[FH-EXEC] calling _init() to rebuild menu\n");
          this._init();
          printf("[FH-EXEC] _init done, retry=true\n");
          retry = true;
        } else {
          retry = false;
        }
      }
    } while ( retry );
    return loop_result;
  }
  
  function _init() {
    local current_dev_id = ::get_current_title_dev_id();
    local title_list = ::get_package_title_dev_name_list();
    m_titles = [];
    for (local no = 0; no < title_list.len(); no++) {
      local title = ::get_title_item(title_list[no]);
      m_titles.append( ::get_item_string(title["dev_name"]) );
    }
    m_menu = this.create_menu();
  }
  
  // メニュー作成
  // @return メニューのinstance
  function create_menu() {}

  // 選択処理
  // @return null: 選択せず int: title_list内のindex
  function exec_body() {}

  // 選択終了してタイトル切替する前に呼ばれる処理 (FOLDER HACK)
  // Pre-set systemdata regionTag so mode_main_change_title's no-arg
  // request_re_init_emulator() picks up the correct ROM, not the stale one.
  // @return なし
  function before_change_title() {}

  // 選択終了してタイトル切替した後に呼ばれる処理
  // @return なし
  function after_change_title() {}

};
/*
class MenuModeTitleSelect extends MenuModeTitleSelectBase {
  
  // メニュー作成
  // @return メニューのinstance
  function create_menu() {
    ::util_load_script("system/script/mode_select_list.nut");
    local current_dev_id = ::get_current_title_dev_id();
    local current_index = ::get_title_in_pack_index(current_dev_id);
    return ModeSelectList(m_titles, current_index);
  }
  
  // 終了
  // @return なし
  function exec_body() {
    local layer = ScaledLayer();
    layer.visible = true;
    local debugFrame = DebugVersionFrame(layer); // XXX デバッグ用バージョン表示

    local result = m_menu.exec("caption_title_select");
    return null != result ? m_menu.index : null;
  }

};
*/
// --

// XXX タイトルメニュー差し替え用適当class
class MenuModeTitleSelect extends MenuModeTitleSelectBase {

  m_layer = null;
  m_config = null;
  m_lists = null;
  m_lists_index = 0;
  
  // メニュー作成
  // @return メニューのinstance
  function create_menu() {
//  SystemEtc.setClearColor(0xffffffFF);
    SystemEtc.setClearColor(0x00000000);
    m_layer = ScaledLayer();
    m_layer.visible = true;
		m_layer.priority = PRI_DEBUG;
    
//    ::util_load_script("system/script/mode_select_list.nut");
    m_config = ::util_load_config( ::conv_path("config/title_mode_top.psb") );
		printf("[FH-CFG] reload title_mode_top: titleNum=%d titleNumTG=%d items.len=%d s_last_linenp=%d\n",
		       m_config["titleNum"], m_config["titleNumTG"], m_config["items"].len(), s_last_linenp);

		// FOLDER HACK: self-correct lineup/pack mismatch.
		// gameapp's `kill -9` on restart can lose an in-memory autosave, leaving
		// the saved s_last_linenp out of sync with the on-disk pack. The on-disk
		// pack is authoritative (.current is written synchronously by the hook);
		// adjust s_last_linenp to match what the pack actually contains.
		{
			local count_for_current = (s_last_linenp == LINEUP_JP)
			                          ? m_config["titleNum"] : m_config["titleNumTG"];
			local count_for_other   = (s_last_linenp == LINEUP_JP)
			                          ? m_config["titleNumTG"] : m_config["titleNum"];
			if (count_for_current == 0 && count_for_other > 0) {
				local new_linenp = (s_last_linenp == LINEUP_JP) ? LINEUP_US : LINEUP_JP;
				printf("[FH-CFG] mismatch: s_last_linenp=%d count=0 (other=%d). Flipping s_last_linenp to %d.\n",
				       s_last_linenp, count_for_other, new_linenp);
				s_last_linenp = new_linenp;
				m_current_linenp = new_linenp;
				// Saved m_current_index was for the old lineup; reset so we don't
				// land on an arbitrary slot in the flipped pack.
				m_current_index = 0;
				s_last_index = 0;
				::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_75_hardType(s_last_linenp);
				::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_08_last_selectTitle(0);
			}
		}

		//タイトル数をjsonから取得する
		if( s_last_linenp == LINEUP_JP )
		{
			s_titleNum = m_config["titleNum"];
		}
		else
		{
			s_titleNum = m_config["titleNumTG"];
		}
		printf( "s_titleNum = %d\n", s_titleNum );

		if( DEMOVERSISON == 1 )
		{
			s_titleNum = 10;	//タイトル数を減らす
		}
    
    m_lists = [];
    for (local no = 0; no < m_config["items"].len(); no++) {
      m_lists.append( format( "%d. %s", no+1, m_config["items"][no]["name"]) );
    }
    local current_index = 0;
    
    return MenuModeTitleSelectSub(m_config, m_current_index, m_current_sortType,m_current_viweType,m_current_pkgregion,m_current_language, m_current_linenp);
//    return ModeSelectList(m_lists, current_index);
  }
  
  // 終了
  // @return なし
  function exec_body() {
		local debugFrame = null; // XXX デバッグ用バージョン表示
		if ( ( DEMOVERSISON == 0 ) && ( BUILDDATE_DIP == 1 ) )
		{
			debugFrame = DebugVersionFrame(m_layer); // XXX デバッグ用バージョン表示
		}

		if( m_menu != null )
		{
	    local result = m_menu.exec("caption_title_select");
			m_current_index = m_menu.m_current_index;
			m_current_sortType = m_menu.m_current_sortType;
			m_current_viweType = m_menu.m_current_viweType;
			m_current_pkgregion = m_menu.m_current_pkgregion;
			m_current_language = m_menu.m_current_language;
			m_current_linenp = m_menu.m_current_linenp;
			m_isSelectRun = m_menu.m_isSelectRun;
			m_selectCount = m_menu.m_selectCount;

			s_last_index = m_current_index;
			s_last_sortType = m_current_sortType;
			s_last_viweType = m_current_viweType;
			s_last_pkgregion = m_current_pkgregion;
			s_last_language = m_current_language;
			s_last_linenp = m_current_linenp;

			//選択したゲームとソート、表示タイプの保存
			::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_07_last_sortType(s_last_sortType);
			::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_08_last_selectTitle(s_last_index);
			::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_09_last_viweType(s_last_viweType);
			::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_75_hardType(s_last_linenp);

			::g_saveIndex = 0;	//セーブ箇所の初期化

	    if ( null != result ) {


				local index = getlastIndex( m_current_index );
//				printf( "--------------------------------------\n" );
//				printf( "getlastIndex(%d) = %d\n", m_current_index, index );
//				printf( "--------------------------------------\n" );
				
				local indexoffset = getLanguageIndexOffset();	//起動するリージョンによってオフセットを加える
	      m_lists_index = index + indexoffset;

		    printf( "m_lists_index = %d\n", m_lists_index );

				if( result == false )
				{
					// プレイデモを起動させる
					return -1;
				}

	      local item = m_config["items"][m_lists_index];
	      for (local no = 0; no < m_titles.len(); no++) {
	        if ( item["dev_name"] == m_titles[no] ) {
	          return no;
	        }
	      }
	    }
	  }
    return null;
  }
  
  // FOLDER HACK: resolve the regionTag for the currently-selected slot
  // by re-reading title_mode_top from disk (force_reload bypasses cache),
  // honoring SELECT-modifier variants (E / S1 / S2).
  function _resolve_selected_regionTag() {
		local index = getlastIndex( m_current_index );
		local indexoffset = getLinenpIndexOffset();
		local lists_index = index + indexoffset;
		local cur_config = ::util_load_config( ::conv_path("config/title_mode_top.psb") );
		local item = cur_config["items"][lists_index];
		local regionTag = item["regionTag"];
		if ( m_isSelectRun == true ) {
			if ( item["regionTagE"] != "NO" ) regionTag = item["regionTagE"];
		}
		else if ( m_selectCount == 2 ) {
			if ( item["regionTagS1"] != "NO" ) regionTag = item["regionTagS1"];
		}
		else if ( m_selectCount == 3 ) {
			if ( item["regionTagS2"] != "NO" ) regionTag = item["regionTagS2"];
		}
		printf("[FH-LAUNCH] resolved lists_index=%d regionTag=%s name=%s\n",
		       lists_index, regionTag, item["name"]);
		return regionTag;
  }

  // FOLDER HACK: pre-set the systemdata regionTag *before*
  // mode_main_change_title's no-arg request_re_init_emulator() fires —
  // otherwise it init's the stale BACK card / wrong game and hangs.
  function before_change_title() {
    local regionTag = this._resolve_selected_regionTag();
    ::g_systemdata.get_value(SystemDataValueIndex.SETTING_ETC).set_game_regionTag(regionTag);
    printf("[FH-LAUNCH] before_change_title: set regionTag=%s\n", regionTag);
  }

  function after_change_title() {
    local regionTag = this._resolve_selected_regionTag();
    ::g_systemdata.get_value(SystemDataValueIndex.SETTING_ETC).set_game_regionTag(regionTag);
    ::request_re_init_emulator(regionTag);
    printf("[FH-LAUNCH] after_change_title: re-init with regionTag=%s\n", regionTag);
  }
};

// XXX タイトルメニュー差し替え用適当class
class MenuModeTitleSelectSub {

	m_rsc = null;
  m_config = null;
  m_configMenu = null;

  m_current_index = null;
  m_current_sortType = null;
  m_current_viweType = null;
  m_current_pkgregion = null;
  m_current_language = null;
  m_current_linenp = null;

  m_layer = null;
  m_layer_down = null;
  m_layer_back = null;
  m_layer_neckun = null;

	m_motion_bg = null;		//背景
	m_motion_bg_down = null;		//背景
	m_motion_bg_back = null;		//背景
	m_selectPkg = null;
  m_pceKun = null;

	m_linenpChangeIn = null;
	m_linenpChangeOut = null;

	m_time = null;
	m_keywait = null;		//キー入力ウエイト
	m_execwait = null;		//ゲーム実行待ち

	
  m_debuglayer = null;
	m_debugtext = null;

	m_indexSetting = null;		//
	m_indexSettingMode = null;		//

	m_guide = null;
  m_layerText = null;

	m_isSelectRun = null;		//
	m_selectCount = null;		//
	m_fukidashiCount = null;		//
	m_text_Setting = null;
	m_text_Sort = null;

	m_titletest_count = null;
	m_lineuptest_count = null;
	
  constructor(config, current_index,current_sortType,current_viweType,current_pkgregion, current_language, current_linenp) 
  {
		m_config = config;
    m_configMenu = ::util_load_config( ::conv_path("config/mode_title_select.psb") );
  	m_current_index = current_index;
		if( m_current_index >= s_titleNum )
		{
			m_current_index = s_titleNum - 1;
		}

  	m_current_sortType = current_sortType;
  	m_current_viweType = current_viweType;
	  m_current_pkgregion = current_pkgregion;
	  m_current_language = current_language;
	  m_current_linenp = current_linenp;

    local motion_path = conv_path(getMotionPath());
    printf("[FH-SUB] ctor: m_current_linenp=%d motion_path=%s items.len=%d\n",
           m_current_linenp, motion_path, m_config["items"].len());

    m_rsc = Resource();
    m_rsc.load( motion_path );
    while (m_rsc.loading)
      wait(0);
    printf("[FH-SUB] ctor: m_rsc loaded\n");
  }

	function _init()
	{
		::g_demo_control.init();

		m_time = 0;
		m_keywait = 0;		//キー入力ウエイト
		m_execwait = 0;
		m_indexSetting = 0;		//
		m_indexSettingMode = 0;		//
		m_isSelectRun = false;		//
		m_selectCount = 0;		//
		m_fukidashiCount = 0;		//
		m_titletest_count = 0;
		m_lineuptest_count = 0;


    m_layer = ScaledLayer();
    m_layer.visible = true;
//    m_layer.smoothing = true;
    m_layer.smoothing = MOTSMOOTHING;
		m_layer.priority = PRI_BG;
    m_layer.registerMotionResource(s_rsc.find(s_ui_motionPath));  // レイヤにモーションリソースを登録
	
    m_layer_down = ScaledLayer();
    m_layer_down.visible = true;
    m_layer_down.smoothing = false;
		m_layer_down.priority = PRI_BG_DOWN;
    m_layer_down.registerMotionResource(s_rsc.find(s_ui_motionPath));  // レイヤにモーションリソースを登録

    m_layer_back = ScaledLayer();
    m_layer_back.visible = true;
    m_layer_back.smoothing = false;
		m_layer_back.priority = PRI_BG_BACK;
    m_layer_back.registerMotionResource(s_rsc.find(s_ui_motionPath));  // レイヤにモーションリソースを登録

    m_layer_neckun = ScaledLayer();
    m_layer_neckun.visible = true;
    m_layer_neckun.smoothing = false;
		m_layer_neckun.priority = PRI_NECKUN;
    m_layer_neckun.registerMotionResource(s_rsc.find(s_ui_motionPath));  // レイヤにモーションリソースを登録

		//モーションの読み込み
    local font_path = conv_path(MENU_MULTI_FONT_PATH);


		if( s_linenp_reboot == true )
		{
//			m_linenpChangeOut = LinenpChange(false);	//ラインナップ切替フェードを切ってみる
		}
		
		//BG作成
		m_motion_bg         = Motion(m_layer);
		m_motion_bg.chara   = getBGMotionName();
		m_motion_bg.motion  = "main";
		m_motion_bg.opacity = 255;
		m_motion_bg.visible = false;
		m_motion_bg.independentLayerInherit = true;
		m_motion_bg.progress();
		m_motion_bg.setVariable("setting_icon_ani", 1);
		m_motion_bg.setVariable("sort_icon_ani", 1);
		
		m_motion_bg.top = -1 * getFrameOffset( );

		
		m_motion_bg_down         = Motion(m_layer_down);
		m_motion_bg_down.chara   = getBGMotionName();
		m_motion_bg_down.motion  = "main_down";
		m_motion_bg_down.opacity = 255;
		m_motion_bg_down.visible = false;
		m_motion_bg_down.independentLayerInherit = true;
		m_motion_bg_down.progress();
		m_motion_bg_down.setVariable("lineup", m_current_linenp);
		m_motion_bg_down.setVariable("setting_icon_ani", 1);
		m_motion_bg_down.setVariable("sort_icon_ani", 1);


		if ( m_current_linenp == 0 )
		{
			m_motion_bg.setVariable("lineup", 1 );
			m_motion_bg_down.setVariable("lineup", 1);
		}
		else
		{
			m_motion_bg.setVariable("lineup", 0 );
			m_motion_bg_down.setVariable("lineup", 0);
		}

		m_motion_bg_down.top = getFrameOffset( );

		m_motion_bg_back = [];
    local i = 0;
    for ( i = 0; i < BGBACK_NUM; i++ )
    {
			m_motion_bg_back.append( Motion(m_layer_back) );
			m_motion_bg_back[i].chara   = getBGMotionName();
			m_motion_bg_back[i].motion  = "back";
			m_motion_bg_back[i].opacity = 255;
			m_motion_bg_back[i].visible = true;
			m_motion_bg_back[i].independentLayerInherit = true;
			m_motion_bg_back[i].progress();
		}

		m_selectPkg = MenuModeSelectPkg( m_config, m_motion_bg, m_motion_bg_down, m_rsc );
		m_selectPkg.setImageID( m_current_sortType );
		m_selectPkg.setIndex(m_current_index);

    m_layerText = ScaledLayer(::util_get_menu_layerFolder());
    m_layerText.visible = true;
    m_layerText.smoothing = true;
		m_layerText.priority = PRI_TEXT;

		//ＰＣＥ君
		local FrameEffectStting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_76_frameAnime( );	//セーブデータから取得する
		m_pceKun = [];
    for ( i = 0; i < NECKUN_NUM; i++ )
    {
			m_pceKun.append( PceKun( m_layer_neckun, FrameEffectStting ) );
		}
/*		
		//パッケージオブジェクトの作成
    m_motion_pkg = [];
    local id = 0;
//    for ( id = 0; id < getTITLE_NUM(); id++ )
    for ( id = 0; id < 50; id++ )
    {
			local ofs = getLanguageIndexOffset( );
	    m_motion_pkg.append( MenuModeTitleSelectPackage(m_config["items"][id + ofs]["image"], m_current_pkgregion, null, m_rsc) );
			m_motion_pkg[id].setLayerPriority(PRI_PKG);

//			printf("id = %d regionTag = %s\n", id, m_config["items"][id]["regionTag"]);
		}
*/

		

		m_motion_bg.visible = true;
		m_motion_bg_down.visible = true;
		
		if( s_menu_reboot == true )
		{
			//メニュー再起動の場合は設定にカーソルを合わせておく
			m_indexSetting = 0;		//
			m_indexSettingMode = 1;		//
			m_motion_bg.setVariable("select", m_indexSetting+1);
			m_motion_bg.setVariable("setting_icon_ani", 0);
			m_motion_bg_down.setVariable("select", m_indexSetting+1);
			m_motion_bg_down.setVariable("setting_icon_ani", 0);
			m_selectPkg.setCursolVisible(false);
			createText( 1 );
		}
		else
		{
			m_motion_bg.setVariable("select", 0);
			m_motion_bg_down.setVariable("select", 0);
			createText( 0 );
		}
		s_menu_reboot = false;
		s_linenp_reboot = false;
		m_selectPkg.setVisible( true );
		
		setTextOffset( );
		setFukidashiCount(  );
		setSortType( m_current_sortType );
	}

	function exec( caption )
	{
		local i = 0;
    local loop_result = false;
    this._init();
		local loopexit = true;
		local reboot = false;

    do {
			
			m_motion_bg.top = -1 * getFrameOffset( );
			m_motion_bg_down.top = getFrameOffset( );

			if( TITLESELECTTEST != 0 )
			{
				//デバッグタイトル連続選択
				m_titletest_count++;	
			}

			if( LINEUPCHANGETEST != 0 )
			{
				//デバッグラインナップ連続切替
				m_lineuptest_count++;	
			}
			
			
			if( m_keywait > 0 )
			{
				m_keywait--;
			}

			if( m_indexSettingMode == 0 )
			{
				//パッケージ選択
				if( m_keywait == 0 )
				{
					if ( ::g_input.key(KEY.UP) ) 
					{
						m_keywait = KEYWAIT;		//キー入力ウエイト
						m_selectCount = 0;		//
					}
					else if ( ::g_input.key(KEY.DOWN) || ( m_lineuptest_count == 20 ) )
					{
						if ( DEMOVERSISON != 0 )
						{
							//デモバージョンでは選択できない
						}
						else
						{
							m_keywait = KEYWAIT;		//キー入力ウエイト
							m_indexSettingMode = 1;		//パッケージ選択へ
							setFukidashiCount(  );

							::g_menu_sound.on_move_cursor(); // SE再生
							m_motion_bg.setVariable("select", m_indexSetting+1);
							m_motion_bg_down.setVariable("select", m_indexSetting+1);
							m_selectPkg.setCursolVisible(false);
							createText( 1 );
							setFukidashiCount(  );
							m_selectCount = 0;		//
						}
					}
					else if ( ::g_input.key(KEY.LEFT) || ( m_titletest_count == 10 ) ) 
					{
						m_keywait = KEYWAIT_PKG;		//キー入力ウエイト
						m_selectPkg.lKey();
						m_selectCount = 0;		//
					}
					else if ( ::g_input.key(KEY.RIGHT) ) 
					{
						m_keywait = KEYWAIT_PKG;		//キー入力ウエイト
						m_selectPkg.rKey();
						m_selectCount = 0;		//
					}
					else if ( ::g_input.keyPressed(KEY.SELECT) ) 
					{
						local index = m_selectPkg.getIndex();
						local tag1 = getLinenpOffsetConfigData(index, "regionTagS1");
						local tag2 = getLinenpOffsetConfigData(index, "regionTagS2");
						local sekectmax = 0;

						if ( tag1 == "NO" )
						{
							sekectmax = 0;
						}
						else if ( ( tag1 != "NO" ) && ( tag2 == "NO" ) )
						{
							sekectmax = 2;
						}
						else
						{
							sekectmax = 3;
						}
						if( sekectmax > 0 )
						{
							if ( ( m_selectCount > 0 ) && ( m_selectCount < sekectmax ) )
							{
								::g_menu_sound.on_decide(); // SE再生
							}
							m_selectCount++;		//
							if( m_selectCount > sekectmax )
							{
								m_selectCount = 0;
							}
						}

					}
				}
				//カーソル位置を更新してから決定処理をする
				if ( m_keywait == 0 )
				{
					if ( ::g_input.keyPressed(KEY.A) || ::g_input.keyPressed(KEY.START) || ( m_titletest_count > 60 ) ) 
					{
						m_current_index = m_selectPkg.getIndex();

						local index = getlastIndex( m_current_index );
						if( DEMOVERSISON == 1 )
						{
							//デモバージョンではすべてHuカードにする
							local sordemo = getLinenpOffsetConfigData(m_current_index, "sor_demo");
							if( sordemo > 2 )
							{
								index = LINEUPMAX;
							}
						}
						if(index != LINEUPMAX )
						{
							::g_menu_sound.on_decide(); // SE再生
							//決定を選択
								m_keywait = KEYWAIT;		//キー入力ウエイト
		          ::g_menu_sound.pause_bgm(true);	//BGMフェード

							local csize = getLinenpOffsetConfigData(m_current_index, "csize");
							printf( "m_current_index = %d, csize = %d\n",m_current_index, csize );

							// FOLDER HACK: csize 10 = enter folder, 11 = exit folder.
							// The native (LD_PRELOAD ::enterGameFolder / ::exitGameFolder)
							// performs the on-disk swap synchronously and returns. The
							// rebuild that follows reloads the swapped PSBs (the cache
							// is bypassed via force_reload in utils.nut).
							if (csize == 10 || csize == 11) {
								// Fade the outgoing pack out before the swap. The rebuild
								// below constructs a fresh m_selectPkg with fadeIn=true so
								// the new pack fades up symmetrically.
								if (m_selectPkg != null) m_selectPkg.fadeOut();

								// Folder navigation. The LD_PRELOAD-installed natives perform
								// the on-disk swap synchronously and return. The rebuild that
								// follows re-reads the swapped PSBs.
								local tag = getLinenpOffsetConfigData(m_current_index, "regionTag");
								if (csize == 10) {
									::enterGameFolder(tag);
									::s_in_folder = true;
								} else {
									::exitGameFolder();
									::s_in_folder = false;
								}

								// Refresh title_prof and everything derived from it. Required
								// because each pack's title_prof has its own indices in
								// game_versions[*][0]; SRAM/save lookups go through
								// s_gameRegionTags which is built once at boot.
								//
								// Order matters:
								//  1. release the script-level merged copy
								//  2. release the ResourceCache that pins title_prof.psb
								//     ACTIVE in the engine — without this, clearResourceCache
								//     can't evict it (it only drops idle entries)
								//  3. release derived state (s_gameRegionTags, etc.)
								//  4. clear the engine cache so next read goes to disk
								//  5. re-run init which rebuilds everything from the
								//     now-current on-disk title_prof
								printf("[FH-REFRESH] step A: null s_current_title_prof / s_rsc_title\n");
								::s_current_title_prof = null;
								::s_rsc_title = null;
								// s_loaded_config (utils.nut) holds a ResourceCache per loaded
								// PSB path. Even with force_reload=true, the OLD cache entry
								// stays alive until we explicitly delete the slot — and while
								// it's alive, title_prof.psb stays ACTIVE in the engine and
								// clearResourceCache can't evict it. Drop the relevant slots.
								printf("[FH-REFRESH] step A2: drop s_loaded_config slots\n");
								local tp_path  = ::conv_path("config/title_prof.psb");
								local tps_path = ::conv_path("config/title_prof_specdepend.psb");
								local tmt_path = ::conv_path("config/title_mode_top.psb");
								if (tp_path  in ::s_loaded_config) ::s_loaded_config.rawdelete(tp_path);
								if (tps_path in ::s_loaded_config) ::s_loaded_config.rawdelete(tps_path);
								if (tmt_path in ::s_loaded_config) ::s_loaded_config.rawdelete(tmt_path);
								printf("[FH-REFRESH] step B: _del_game_content_config\n");
								try { ::_del_game_content_config(); printf("[FH-REFRESH] step B ok\n"); }
								catch (e) { printf("[FH-REFRESH] step B EXC: %s\n", e); }
								printf("[FH-REFRESH] step C: System.clearResourceCache\n");
								try { System.clearResourceCache(); printf("[FH-REFRESH] step C ok\n"); }
								catch (e) { printf("[FH-REFRESH] step C EXC: %s\n", e); }
								printf("[FH-REFRESH] step D: _init_game_content_config\n");
								try { ::_init_game_content_config(); printf("[FH-REFRESH] step D ok\n"); }
								catch (e) { printf("[FH-REFRESH] step D EXC: %s\n", e); }

								// Re-read top-level config from the now-swapped file
								// (utils.nut sets force_reload=true, so the script-level
								// PSB cache is bypassed).
								m_config = ::util_load_config(::conv_path("config/title_mode_top.psb"));
								if (s_last_linenp == LINEUP_JP) s_titleNum = m_config["titleNum"];
								else                            s_titleNum = m_config["titleNumTG"];
								m_keywait = KEYWAIT;

								local motion_path = conv_path(getMotionPath());

								// Release the OLD m_selectPkg first — its m_layer.registerMotionResource()
								// holds a refcount on the cover-sheet resource, keeping it in the
								// engine's "active" list. Without this, m_rsc.unload() leaves the
								// resource active and System.clearResourceCache() can't evict it
								// (it only drops IDLE cache entries). Order matters here.
								m_selectPkg = null;
								m_rsc.unload();
								m_rsc = null;
								System.clearResourceCache();

								m_rsc = Resource();
								m_rsc.load(motion_path);
								while (m_rsc.loading) wait(0);

								// fadeIn=true (default) so folder enter/exit gets the same
								// fade-up animation as fresh menu construction.
								m_selectPkg = MenuModeSelectPkg(m_config, m_motion_bg, m_motion_bg_down, m_rsc);
								m_selectPkg.setImageID( m_current_sortType );
								m_selectPkg.setIndex( (csize == 10) ? 1 : 0 );  // enter: skip back card
								m_selectPkg.setVisible( true );
							}
							else {
							//演出待ち時間
							m_isSelectRun = false;		//SELECTも押されている
							//スーパーシステムカードをシステムカードに変える
							if( ::g_input.key(KEY.SELECT) )
							{
								m_isSelectRun = true;		//SELECTも押されている
							}

							m_selectPkg.setLineup( m_current_linenp );
							m_indexSettingMode = 2;		//ゲーム起動へ

							local screenMode =  getLinenpOffsetConfigData(m_current_index, "ScreenMode");
							::g_emu_task.SetScreenMode( screenMode );	//個別に画面サイズ対応
							local screenOfsX =  getLinenpOffsetConfigData(m_current_index, "EmuOfsX");
							local screenOfsY =  getLinenpOffsetConfigData(m_current_index, "EmuOfsY");
							::g_emu_task.SetEmuScreenOffsetX( screenOfsX );	//個別に画面サイズ対応
							::g_emu_task.SetEmuScreenOffsetY( screenOfsY );	//個別に画面サイズ対応
	//						printf( "screenMode = %d\n", screenMode );

							//ステートセーブのサイズを設定する
							switch( csize )
							{
							case 2:
							case 3:
							case 4:
							case 5:	//沙羅曼蛇専用
								//CDタイトル
								::setBackupStatedataSegmentType(BackupSegmentTypes.EMU_STATE_L);
								break;
							default:
								//huカード
								//SGカード
								::setBackupStatedataSegmentType(BackupSegmentTypes.EMU_STATE);
								break;
							}
							} // end else (non-folder game launch)
						}
						else
						{
							//選択できない
							::g_menu_sound.on_back(); // SE再生
						}
						printf( "m_current_index = %d, index = %d\n",m_current_index, index );
					}
				}
			}
			else if ( m_indexSettingMode == 1 )
			{
				//設定画面選択
				if( m_keywait == 0 )
				{
					if ( ::g_input.key(KEY.UP) ) 
					{
						m_keywait = KEYWAIT;		//キー入力ウエイト
						m_indexSettingMode = 0;		//パッケージ選択へ
						::g_menu_sound.on_move_cursor(); // SE再生
						m_motion_bg.setVariable("select", 0);
						m_motion_bg.setVariable("disable", 0);
						m_motion_bg_down.setVariable("select", 0);
						m_motion_bg_down.setVariable("disable", 0);
						m_motion_bg_down.setVariable("sortwin", 0);
						m_selectPkg.setCursolVisible(true);
						createText( 0 );
						setFukidashiCount(  );
					}
					else if ( ::g_input.key(KEY.DOWN) ) 
					{
						m_keywait = KEYWAIT;		//キー入力ウエイト
					}
					else if ( ::g_input.key(KEY.LEFT) || ( m_lineuptest_count == 40 ) ) 
					{
						m_keywait = KEYWAIT;		//キー入力ウエイト
						::g_menu_sound.on_move_cursor(); // SE再生
						
						m_indexSetting--;
						if(m_indexSetting < 0)
						{
							m_indexSetting = 2;
						}
						m_motion_bg.setVariable("select", m_indexSetting+1);
						m_motion_bg_down.setVariable("select", m_indexSetting+1);
						setFukidashiCount(  );
					}
					else if ( ::g_input.key(KEY.RIGHT) ) 
					{
						m_keywait = KEYWAIT;		//キー入力ウエイト
						::g_menu_sound.on_move_cursor(); // SE再生

						m_indexSetting++;
						if(m_indexSetting > 2)
						{
							m_indexSetting = 0;
						}
						m_motion_bg.setVariable("select", m_indexSetting+1);
						m_motion_bg_down.setVariable("select", m_indexSetting+1);
						setFukidashiCount(  );
					}
				}
				
				//カーソル位置を更新してから決定処理をする
				if ( m_keywait == 0 )
				{
					if ( ::g_input.keyPressed(KEY.A) || ( m_lineuptest_count == 60 ) ) 
					{
						::g_menu_sound.on_enter(); // SE再生
						switch( m_indexSetting )
						{
						case 0:
							//設定
//							m_motion_bg.setVariable("select", 0);
							s_last_language = m_current_language;
							printf("s_last_language1=%d\n", s_last_language );

							m_selectPkg.setVisible( false );
							m_selectPkg.exec();

					    for ( i = 0; i < NECKUN_NUM; i++ )
					    {
								m_pceKun[i].setVisible( false );
							}

							local frameTypeOld = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_74_frameType( );
							//変更前のタグを取得する
							local current_index = m_selectPkg.getIndex();
							local tagold =  getLinenpOffsetConfigData(current_index, "regionTag");

							::util_load_script("system/script/mode_title_select_setting.nut");
							::mode_title_select_setting_main( );	//設定画面メイン

							local fseq = ::g_frameCount.getFinishSeq();
					    if ( fseq > 0 ) 
					    {
								//シャットダウンで抜けた場合はリソース読み変えずに終了する
								::g_frameCount.logFileWrite("mode_title_select_setting_main power off");
							}
							else
							{
/*
								if( m_current_language != ::getSettingLanguage() )
								{
									loopexit = false;	//別の言語になったらメインループを抜ける
									reboot = true;
									s_menu_reboot = true;
									m_current_language = ::getSettingLanguage();
									break;	//直接ループを抜ける
								}
*/
								if( getSettingIsInitlize() == true )
								{
									setFrameinOut( );

									m_current_sortType = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_07_last_sortType();
									m_current_index = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_08_last_selectTitle();
									m_current_linenp = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_75_hardType();
									m_current_language = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_06_last_language();
									
									loopexit = false;	//別の言語になったらメインループを抜ける
									reboot = true;
//									s_menu_reboot = true;
//									m_current_language = ::getSettingLanguage();
									break;	//直接ループを抜ける
								}
								if ( frameTypeOld != ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_74_frameType( ) )
								{
									if ( s_last_linenp == LINEUP_JP )
									{
		//								m_motion_bg.setVariable("select", m_settingselect+1);
										setFrameinOut( );
										m_motion_bg.chara   = getBGMotionName();
										m_motion_bg_down.chara = getBGMotionName( );

										m_motion_bg.motion  = "main";
										m_motion_bg_down.motion  = "main_down";

								    local i = 0;
								    for ( i = 0; i < BGBACK_NUM; i++ )
								    {
											m_motion_bg_back[i].chara   = getBGMotionName();
											m_motion_bg_back[i].motion  = "back";
										}
									}
								}

								m_current_language = ::getSettingLanguage();

								setSortType( m_current_sortType );

								//同じタグの所にカーソルを移動する
								local i = 0;
								for( i = 0; i < LINEUPMAX; i++ )
								{
									local tag =  getLinenpOffsetConfigData(i, "regionTag");
									if( tagold == tag )
									{
										m_selectPkg.setIndex(i);
										break;
									}
								}
								m_selectPkg.setVisible( true );
								local FrameEffectStting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_76_frameAnime( );	//セーブデータから取得する
						    for ( i = 0; i < NECKUN_NUM; i++ )
						    {
									m_pceKun[i].setVisible( FrameEffectStting );
								}


								//ガイドを作り直す
								createText( 1 );
								setFukidashiCount(  );
								m_selectPkg.nameTextUpdate();	//タイトル名を言語に合わせた内容で更新する

								printf("s_last_language2=%d\n", s_last_language );
							}
							break;
						case 1:
							//ソート
							switch( m_current_sortType )
							{
								case SORTTYPE_NAME:
									m_current_sortType = SORTTYPE_GENRE;
									break;
								case SORTTYPE_DATE:
									m_current_sortType = SORTTYPE_NAME;
									break;
								case SORTTYPE_GENRE:
//									m_current_sortType = SORTTYPE_PLNUM;
									m_current_sortType = SORTTYPE_DATE;
									break;
								case SORTTYPE_PLNUM:
									m_current_sortType = SORTTYPE_DATE;
									break;
							}

							//変更前のタグを取得する
							local current_index = m_selectPkg.getIndex();
							local tagold =  getLinenpOffsetConfigData(current_index, "regionTag");

							setSortType( m_current_sortType );

							//同じタグの所にカーソルを移動する
							local i = 0;
							for( i = 0; i < LINEUPMAX; i++ )
							{
								local tag =  getLinenpOffsetConfigData(i, "regionTag");
								if( tagold == tag )
								{
									m_selectPkg.setIndex(i);
									break;
								}
							}
							m_selectPkg.setVisible( true );
//							m_selectPkg.nameTextUpdate();
							break;
						case 2:
							//タイトル変更
							if (!can_enter_lineup((m_current_linenp == LINEUP_JP) ? LINEUP_US : LINEUP_JP)) {
								::g_menu_sound.on_ng();
								m_keywait = KEYWAIT;
								break;
							}
							::g_menu_sound.on_power_off();
							{
								// FOLDER HACK: per-lineup cursor memory (root-only).
								// m_current_index doesn't track scrolling — only updates on A
								// or demo entry — so read the live cursor from m_selectPkg.
								// If we're inside a folder, the live cursor is for the folder
								// pack, NOT the lineup root. Don't pollute the per-lineup root
								// memory with a folder cursor; leave the saved value alone.
								// The lineup toggle always swaps to <new>/_root (not into a
								// folder), so on incoming we always restore the root cursor.
								local outgoing = m_current_linenp;
								local incoming = (m_current_linenp == LINEUP_JP) ? LINEUP_US : LINEUP_JP;
								local live_idx = m_selectPkg.getIndex();
								if (!::s_in_folder) {
									::s_last_index_per_linenp[outgoing] = live_idx;
									printf("[FH-LINEUP] save root idx[%d]=%d\n", outgoing, live_idx);
								} else {
									printf("[FH-LINEUP] in folder, skip save (live=%d)\n", live_idx);
								}
								// Toggle leaves any folder context — we're heading to <new>/_root.
								::s_in_folder = false;
								local restored = ::s_last_index_per_linenp[incoming];
								printf("[FH-LINEUP] restore root idx[%d]=%s\n",
								       incoming, restored == null ? "null" : restored.tostring());
								m_current_index = (restored != null) ? restored : 0;
								m_current_linenp = incoming;
							}

							// FOLDER HACK: swap on-disk to the new lineup's _root pack
							// before s_linenp_reboot triggers the menu rebuild. We only
							// drop the lightweight caches here — _del/_init_game_content_config
							// re-initialize the emulator and freeze the title-select frame
							// loop, so we defer those to the post-reboot init() path.
							{
								local new_lineup = (m_current_linenp == LINEUP_JP) ? "jp" : "us";
								local tag = "FOLDER_" + new_lineup + "__root";
								printf("[FH-LINEUP] toggle to %s, calling enterGameFolder(%s)\n", new_lineup, tag);
								::enterGameFolder(tag);
								::s_current_title_prof = null;
								::s_rsc_title = null;
								local tp_path  = ::conv_path("config/title_prof.psb");
								local tps_path = ::conv_path("config/title_prof_specdepend.psb");
								local tmt_path = ::conv_path("config/title_mode_top.psb");
								if (tp_path  in ::s_loaded_config) ::s_loaded_config.rawdelete(tp_path);
								if (tps_path in ::s_loaded_config) ::s_loaded_config.rawdelete(tps_path);
								if (tmt_path in ::s_loaded_config) ::s_loaded_config.rawdelete(tmt_path);
								::s_lineup_pack_dirty <- true;
								// FOLDER HACK: ModeDemo caches m_config at init only, so
								// without this the demo keeps picking games from the lineup
								// that was active at boot.
								if (::g_demo_control != null) {
									::g_demo_control.reload_config();
									printf("[FH-LINEUP] g_demo_control.reload_config()\n");
								}
								printf("[FH-LINEUP] swap done, marked dirty\n");
							}

/*
							loopexit = false;	//別の言語になったらメインループを抜ける
							reboot = true;
//							s_menu_reboot = true;
							break;
*/
							//タイトル切り替え演出
							m_indexSettingMode = 3;
							m_linenpChangeIn = LinenpChange(true);
							s_linenp_reboot = true;
							m_selectPkg.setVisible(false);

		          ::g_menu_sound.pause_bgm(true);	//BGMフェード
						}
					}
				}
			}
			else if ( m_indexSettingMode == 2 )
			{
				//ゲーム起動演出
				m_execwait++;

				local csize = getLinenpOffsetConfigData(m_current_index, "csize");

				//演出待ち時間
				local wait = 0;

				local startoffset = 30;
				if( m_execwait == startoffset )
				{
					m_selectPkg.gameStart(m_isSelectRun);
				}
				switch( csize )
				{
				case 0:
				case 5:
					//huカード
					wait = 120 + startoffset;
					if(m_execwait == ( 20 ) )
					{
						::g_menu_sound.on_open_cardcase(); // SE再生
					}
					if(m_execwait == ( 35 + startoffset ))
					{
						::g_menu_sound.on_insert_card(); // SE再生
					}
					
					break;
				case 1:
					//SGカード
					wait = 210 + startoffset;
					if(m_execwait == 20 )
					{
						::g_menu_sound.on_case_a();
					}
					if(m_execwait == ( 70 + startoffset ))
					{
						::g_menu_sound.on_open_cardcase(); // SE再生
					}
					if(m_execwait == ( 130 + startoffset ))
					{
						::g_menu_sound.on_insert_card(); // SE再生
					}
					break;
				case 2:
				case 3:
				case 4:
					//CDタイトル
					wait = 270 + startoffset;
					if(m_execwait == ( 20 ) )
					{
						::g_menu_sound.on_open_cardcase(); // SE再生
					}
					if(m_execwait == ( 35 + startoffset ))
					{
						::g_menu_sound.on_insert_card(); // SE再生
					}
					if(m_execwait == ( 90 + startoffset ))
					{
						::g_menu_sound.on_insert_cd(); // SE再生
					}
					if(m_execwait == ( 140 + startoffset ))
					{
//						::g_menu_sound.on_cd_seek(); // SE再生
					}
					break;
				}

				if ( ( m_execwait > wait ) || ( ( m_execwait > 10 ) && ( ::g_input.keyPressed(KEY.A) ) ) )
				{
					loopexit = false;
					//決定モーション
				}
			}
			else if ( m_indexSettingMode == 3 )
			{
				//ラインナップ切替演出
				m_execwait++;
//				if ( ( m_execwait > LINENUP_WAIT ) || ( ::g_input.keyPressed(KEY.A) ) )
				if ( m_execwait > LINENUP_WAIT )
				{
					//タイトル変更
					loopexit = false;	//別の言語になったらメインループを抜ける
					reboot = true;
//				s_menu_reboot = true;
					setFrameinOut( );
				}
			}

			m_selectPkg.exec();
			FrameinOutExec( );
			setTextOffset( );
			FukidashiExec( );

			
	    for ( i = 0; i < NECKUN_NUM; i++ )
	    {
				m_pceKun[i].exec();
			}

//			::g_wallpaper.close();
			::delete_wallpaper();
//			::g_wallpaperBG.close();
			::delete_wallpaperBG();

			if( m_time > 30 )
			{
				m_linenpChangeOut = null;
			}

	    //パワーボタン検出
			local fseq = ::g_frameCount.getFinishSeq();
	    if ( fseq > 0 ) 
	    {
				printf("finishifunc 1\n");
				//保存する
				::g_frameCount.setFinishSeq(2);	//シャットダウン要求

				printf("finishifunc 2\n");
				::g_frameCount.appFinish();	//シャットダウン

//	      break; // モード終了
			}
			else if( DEMO_USE != 0 )
			{
				if ( DEMOVERSISON == 0 )
				{
//				if( m_curmode == 0 )	//パッケージ選択
					{
						if( ::g_demo_control.check(this) )
						{
							m_current_index = m_selectPkg.getIndex();	//デモに入った時のカーソル位置を保存
							return( false );
						}
					}
				}
			}

			//背景のスクロール
	    local i = 0;
	    for ( i = 0; i < BGBACK_NUM; i++ )
	    {
				local x = ( i * -SCREEN_XSIZE ) + ( ( ::g_bg_count / BGCOUNT_WARI ) % SCREEN_XSIZE );
//				m_motion_bg_back[i].left = x;
				//イラスト制御

		    local j = 0;
		    for ( j = 1; j <= 8; j++ )
		    {
					local yure = ( ::get_yure( ::g_bg_count + ( j * 20 ) , 50, 4 ) / 100.0 ) + 0.5;
					local val = "bgillustration0" + j.tostring();
					m_motion_bg_back[i].setVariable(val, yure);
				}
/*
					m_motion_bg_back[i].setVariable("bgillustration01", 1.0);
					m_motion_bg_back[i].setVariable("bgillustration02", 0.5);
					m_motion_bg_back[i].setVariable("bgillustration03", 1.0);
					m_motion_bg_back[i].setVariable("bgillustration04", 0.5);
					m_motion_bg_back[i].setVariable("bgillustration05", 1.0);
					m_motion_bg_back[i].setVariable("bgillustration06", 0.5);
					m_motion_bg_back[i].setVariable("bgillustration07", 1.0);
					m_motion_bg_back[i].setVariable("bgillustration08", 0.5);
*/
			}


			::g_bg_count++;
			m_time++;

      wait(0);
    } while ( loopexit );
    
::g_frameCount.logFileWrite("mainloop exit 1");

    if( reboot == false )
    {
::g_frameCount.logFileWrite("mainloop exit 2");
			return( true );
		}
		else
		{
::g_frameCount.logFileWrite("mainloop exit 3");
			::init_wallpaperBG();
			::g_wallpaperBG.open();	//背景を残しておく
			return( null );	//再起動
		}
		
	}

	//ソートタイプの変更
	function setSortType( sortType )
	{
		m_current_sortType = sortType;
		s_last_sortType = sortType;
		m_motion_bg.setVariable("sort", m_current_sortType);
		m_motion_bg_down.setVariable("sort", m_current_sortType);
		m_selectPkg.setImageID(m_current_sortType);

		setFukidashiSortText(  );
	}

	function createText( type )
	{
		//開放する
		m_guide = null;

		local offsetY1 = 0;
		local offsetY2 = 0;
		local textH = 60;

		switch( m_current_language )
		{
			case LANGUAGE_CHA:
			case LANGUAGE_KOR:
			case LANGUAGE_SPA:
			case LANGUAGE_FRA:
			case LANGUAGE_ITA:
			case LANGUAGE_GER:
				offsetY1 = 19;
				offsetY2 = 14;
				break;
			case LANGUAGE_JPN:
			case LANGUAGE_ENG:
				offsetY1 = 19;
				offsetY2 = 14;
				break;
		}

		local textData = null;
		local backY = 240;
		local backIndex = 8
/*
		textData = m_configMenu["LANG"];
		
		{
			local str = get_item_string(textData["SettingMainText__CAPTION"]);

			m_caption = Indicator(m_layerText, rsc.find(font_path1));
			m_caption.visible = true;
			m_caption.setRecognizeTag(true);
			m_caption.fontColor = TEXT_COLOR_NORMAL;
			m_caption.setAlignment(CONSOLE.ALIGNMENT_CENTER);
			m_caption.print(str);
			m_caption.setCoord( 0, -276 - offsetY1);
		}
*/
/*
		{
			local str = get_item_string(textData["SettingMainText__GUIDE"]);

			m_guide = Indicator(m_layerText, rsc.find(font_path2));
			m_guide.visible = true;
			m_guide.setRecognizeTag(true);
			m_guide.fontColor = TEXT_COLOR_NORMAL;
			m_guide.setAlignment(CONSOLE.ALIGNMENT_CENTER);
			m_guide.print(str);
			m_guide.setCoord( 0, 274 - offsetY1);
		}
*/
		local guidenum = 3;
		local textDataGuide = null;
		
		switch( type )
		{
		case 0:
			if( DEMOVERSISON == 1 )
			{
				guidenum = 2;
				textDataGuide = m_configMenu["GUIDE_MAIN_TRAIAL"];
			}
			else
			{
				textDataGuide = m_configMenu["GUIDE_MAIN"];
			}
			break;
		case 1:
			textDataGuide = m_configMenu["GUIDE_OPTION"];
			break;
		}
		
		if( textDataGuide )
		{
			m_guide = [];

			local i = 0;
			for ( i = 0; i < guidenum ; i++ )
			{
				local idxname = "SettingMainIcon__" + i.tostring();
				local str1 = get_item_string(textDataGuide[idxname]);

				idxname = "SettingMainText__" + i.tostring();
				local str2 = get_item_string(textDataGuide[idxname]);
				
				local x = -400;
				local w = 300;
				if( guidenum == 2 )
				{
					x = -350;
					w = 450;
				}
				m_guide.append( MenuModeGuide(m_layerText, x + w * i, 314, str1, str2, 400, true ) );

			}
		}
	}
	
	function setTextOffset( )
	{
		local i = 0;
		for ( i = 0; i < m_guide.len() ; i++ )
		{
			m_guide[i].setOffset( getFrameOffset( ) );
		}
	}
	
	function setFukidashiCount(  )
	{
		m_fukidashiCount = 0;

		if( m_text_Setting == null )
		{
			local font_path = ::getMulti18FontPath( );

			m_text_Setting = Indicator(m_layerText, s_rsc.find(font_path));
			m_text_Setting.visible = false;
			m_text_Setting.setRecognizeTag(true);
			m_text_Setting.fontColor = TEXT_COLOR_NORMAL;
			m_text_Setting.setAlignment(CONSOLE.ALIGNMENT_CENTER);
			
			m_text_Sort = Indicator(m_layerText, s_rsc.find(font_path));
			m_text_Sort.visible = false;
			m_text_Sort.setRecognizeTag(true);
			m_text_Sort.fontColor = TEXT_COLOR_NORMAL;
			m_text_Sort.setAlignment(CONSOLE.ALIGNMENT_CENTER);
		}

		if( m_indexSettingMode == 1 )
		{
			local str = "";

			local textData = null;
			textData = m_configMenu["SETTING_SELECT"];

			switch( m_indexSetting )
			{
			case 0:
				str = get_item_string(textData["SettingHukidashi1"]);
				m_text_Setting.print(str);
				IndicatorSetXScale( m_text_Setting, 200 );
				break;
			case 1:
//				str = get_item_string(textData["SettingHukidashi2"]);
				setFukidashiSortText(  );
				break;
			case 2:
				str = get_item_string(textData["SettingHukidashi3"]);
				m_text_Setting.print(str);
				IndicatorSetXScale( m_text_Setting, 300 );
				break;
			}
		}
		m_text_Setting.visible = false;
		m_text_Sort.visible = false;
	}
	function FukidashiExec(  )
	{
		m_fukidashiCount++;
		
		if( m_fukidashiCount > FUKIDASHI_ANIME )
		{
			m_fukidashiCount = FUKIDASHI_ANIME;
			//テキストを表示する
			if( m_text_Setting != null )
			{
				if( m_indexSettingMode == 1 )
				{
					m_text_Setting.visible = true;
				}
			}
			if( m_text_Sort != null )
			{
				if ( ( m_indexSettingMode == 1 ) && ( m_indexSetting == 1 ) )	//設定のソートにある
				{
					m_text_Sort.visible = true;
				}
			}
		}

		local per = m_fukidashiCount / FUKIDASHI_ANIME;
/*
		if ( ( m_indexSettingMode == 1 ) && ( m_indexSetting == 1 ) )	//設定のソートにある
		{
			m_motion_bg_down.setVariable("sortwin", per);
		}
		else
		{
			m_motion_bg_down.setVariable("sortwin", 0);
		}
*/
		m_motion_bg_down.setVariable("sortwin", 0);

		local max = 1.0;
		//言語別に吹き出しサイズを設定する
/*
const LANGUAGE_JPN = 0;
const LANGUAGE_ENG = 1;
const LANGUAGE_SPA = 2;
const LANGUAGE_FRA = 3;
const LANGUAGE_ITA = 4;
const LANGUAGE_GER = 5;
*/		
		if ( ( m_indexSetting == 0 ) )
		{
			local lanmax = [0.3, 0.3, 0.45, 0.3 0.45, 0.5 ];
			max = lanmax[m_current_language];
		}
		else if ( ( m_indexSetting == 1 ) )
		{
			local lanmax = [0.3, 0.45 0.45 0.45, 0.6, 0.8 ];
			max = lanmax[m_current_language];
			if ( m_current_sortType == SORTTYPE_NAME )
			{
				max = 0.3;
			}
			if ( ( m_current_language == LANGUAGE_FRA ) && ( m_current_sortType == SORTTYPE_GENRE ) )
			{
				max = 0.3;
			}
		}
		else
		{
			local lanmax = [0.6, 0.6, 0.6, 0.6, 0.6, 1.0 ];
			max = lanmax[m_current_language];
		}
		if( per > max )
		{
			per = max;
		}
		

		m_motion_bg_down.setVariable("fukidasi_zoom", per );
		
		
		//テキストの位置更新
		local x = 0;
		local y = 236;
		switch( m_indexSetting )
		{
		case 0:
			local lanx = [430, 430, 430, 430, 430, 430 ];
			x = lanx[m_current_language];
			break;
		case 1:
			local lanx = [465, 445, 445, 445, 430, 400 ];
			x = lanx[m_current_language];
			if ( m_current_sortType == SORTTYPE_NAME )
			{
				x = 465;
			}
			if ( ( m_current_language == LANGUAGE_FRA ) && ( m_current_sortType == SORTTYPE_GENRE ) )
			{
				x = 465;
			}
			break;
		case 2:
			local lanx = [505, 505, 505, 505, 505, 460 ];
			x = lanx[m_current_language];
			break;
		}
		IndicatorSetCoord( m_text_Setting, x, y + getFrameOffset( ) );

		IndicatorSetCoord( m_text_Sort, 320, y + getFrameOffset( ) );
		
	}
	//ソートテキストの設定
	function setFukidashiSortText(  )
	{
		//ソ－トテキスト
		local str = "";
		local textData = null;
		if( s_last_linenp == LINEUP_JP )
		{
			//ラインナップでソートの表記を変える
			textData = m_configMenu["SORT_SELECT"];
		}
		else
		{
			textData = m_configMenu["SORT_SELECT_TG"];
		}
		switch( m_current_sortType )
		{
		case SORTTYPE_NAME:
			//名前順
			str = get_item_string(textData["sortHukidashi1"]);
			break;
		case SORTTYPE_DATE:
			//日付順
			str = get_item_string(textData["sortHukidashi2"]);
			break;
		case SORTTYPE_GENRE:
			//ジャンル
			str = get_item_string(textData["sortHukidashi3"]);
			break;
		case SORTTYPE_PLNUM:
			//プレイ人数
			str = get_item_string(textData["sortHukidashi4"]);
			break;
		}
		
//		m_text_Sort.print(str);
		m_text_Setting.print(str);
		IndicatorSetXScale( m_text_Setting, 320 );
		return( str );
	}
	
}

//320
const PKGNAME_TEXTOFFSET_Y = 169;
const INDEX_LENGTH_MAX = 700;
const PKG_W = 320.0;
const PKG_Y = -10;
const SLIDEWAIT = 5;
const CUNTERINDEX = 2;
const THUMBNAIL_W = 34;
const THUMBNAIL_X = -300;
const THUMBNAIL_Y = -194;
class MenuModeSelectPkg
{
	m_layer = null;
	m_layer_s = null;
	m_layer_cursol = null;
	m_layer_title = null;
	m_config = null;
	m_isVisible = null;
	m_isCursolVisible = null;

	m_count = null;
	m_move = null;
	m_index = null;
	m_sort = null;

	m_pkgselect_work = null;
	m_pkgselect_motion = null;
	m_pkgselect_s_motion = null;

	m_cursol_motion = null;
	m_thumbnail_cursol_motion = null;

	m_text_Title = null;

	m_motion_bg = null;
	m_motion_bg_down = null;

	m_exec_motion = null;
	m_isExec = null;
	m_linenp = null;

	// FOLDER HACK: when rebuilding the menu after a folder swap, we can't run
	// the fade-in animation (no carrier loop to advance the motions). fadeIn=false
	// makes _init() set motion opacity straight to 255 instead of 0.
	m_fadeIn = null;

  constructor( config, motion_bg, motion_bg_down, rsc, fadeIn = true )
  {
		m_fadeIn = fadeIn;
		m_motion_bg = motion_bg;
		m_motion_bg_down = motion_bg_down;
		m_config = config;
		m_index = 0;
		m_count = 0;
		m_move = 0;
		m_isVisible = false;
		m_isCursolVisible = true;
		m_isExec = false;
		m_linenp = 0;

		local font_path = ::getTitleMultiFontPath( );
    local motion_path = conv_path(getMotionPath());

		m_pkgselect_work = [];
    local id = 0;
    for ( id = 0; id < s_titleNum; id++ )
    {
			m_pkgselect_work.append( id );
		}

		m_layer = ScaledLayer();
		m_layer.visible = true;
		m_layer.smoothing = MOTSMOOTHING;
		m_layer.priority = PRI_PKG;
		m_layer.registerMotionResource(rsc.find(motion_path));  // レイヤにモーションリソースを登録
		m_layer.registerMotionResource(s_rsc.find(s_ui_motionPath));  // レイヤにモーションリソースを登録

		m_layer_s = ScaledLayer();
		m_layer_s.visible = true;
		m_layer_s.smoothing = true;
		m_layer_s.priority = PRI_PKG;
		m_layer_s.registerMotionResource(rsc.find(motion_path));  // レイヤにモーションリソースを登録
		
		m_layer_title = ScaledLayer();
		m_layer_title.visible = true;
		m_layer_title.smoothing = true;
		m_layer_title.priority = PRI_TEXT;

		m_layer_cursol = ScaledLayer();
		m_layer_cursol.visible = true;
		m_layer_cursol.smoothing = MOTSMOOTHING;
		m_layer_cursol.priority = PRI_CSL;
		m_layer_cursol.registerMotionResource(s_rsc.find(s_ui_motionPath));  // レイヤにモーションリソースを登録
		
		m_text_Title = Indicator(m_layer_title, s_rsc.find(font_path));
		local _str = "";
		m_text_Title.visible = m_isVisible;
		m_text_Title.setRecognizeTag(true);
		m_text_Title.fontColor = TEXT_COLOR_NORMAL;
		m_text_Title.setAlignment(CONSOLE.ALIGNMENT_CENTER);
		m_text_Title.print(_str);
		m_text_Title.setCoord( 0, ( PKGNAME_TEXTOFFSET_Y - getFont32Yoffset() ) );

		_init();
	}
	function _init()
	{
		m_pkgselect_motion = [];
		m_pkgselect_s_motion = [];
    local i = 0;
    for ( i = 0; i < s_titleNum; i++ )
    {
			m_pkgselect_motion.append( Motion(m_layer) );
			m_pkgselect_motion[i].chara   = "common_parts";
			m_pkgselect_motion[i].motion  = "pkg";
			// FOLDER HACK: skip fade-in when rebuilding after folder swap
			m_pkgselect_motion[i].opacity = m_fadeIn ? 0 : 255;
			m_pkgselect_motion[i].visible = m_isVisible;
			m_pkgselect_motion[i].independentLayerInherit = true;
			m_pkgselect_motion[i].progress();
			m_pkgselect_motion[i].left = 0;	//0,0が画面中央
			m_pkgselect_motion[i].top = 0;
			m_pkgselect_motion[i].setVariable("pkg", 0);

			m_pkgselect_s_motion.append( Motion(m_layer_s) );
			m_pkgselect_s_motion[i].chara   = "pkg";
			m_pkgselect_s_motion[i].motion  = "thumb";
			m_pkgselect_s_motion[i].opacity = 255;
			m_pkgselect_s_motion[i].visible = m_isVisible;
			m_pkgselect_s_motion[i].independentLayerInherit = true;
			m_pkgselect_s_motion[i].progress();
			m_pkgselect_s_motion[i].left = ( THUMBNAIL_W * i ) - ( ( s_titleNum / 2 ) * THUMBNAIL_W );	//0,0が画面中央
			m_pkgselect_s_motion[i].top = THUMBNAIL_Y;
			m_pkgselect_s_motion[i].setVariable("pkg", 0);
		}

		
		m_cursol_motion = Motion(m_layer_cursol);
		m_cursol_motion.chara   = "cursol";
		m_cursol_motion.motion  = "front";
		m_cursol_motion.opacity = 0;
		m_cursol_motion.visible = m_isCursolVisible;
		m_cursol_motion.independentLayerInherit = true;
		m_cursol_motion.progress();
		m_cursol_motion.left = 0;	//0,0が画面中央
		m_cursol_motion.top = PKG_Y;

		m_thumbnail_cursol_motion = Motion(m_layer_cursol);
		m_thumbnail_cursol_motion.chara   = "common_parts";
		m_thumbnail_cursol_motion.motion  = "finger";
		m_thumbnail_cursol_motion.opacity = 0;
		m_thumbnail_cursol_motion.visible = m_isCursolVisible;
		m_thumbnail_cursol_motion.independentLayerInherit = true;
		m_thumbnail_cursol_motion.progress();
		m_thumbnail_cursol_motion.left = 0;	//0,0が画面中央
		m_thumbnail_cursol_motion.top = THUMBNAIL_Y;
	}
	
	function lKey( )
	{
		if( m_move == 0 )
		{
			m_move = 1;
			m_count = SLIDEWAIT;
			::g_menu_sound.on_move_cursor(); // SE再生
		}
	}
	function rKey( )
	{
		if( m_move == 0 )
		{
			m_move = -1;
			m_count = SLIDEWAIT;
			::g_menu_sound.on_move_cursor(); // SE再生
		}
	}

	function move_l( )
	{
		local l = m_pkgselect_work[0];

    local i = 0;
    for ( i = 0; i < s_titleNum - 1; i++ )
    {
			m_pkgselect_work[i] = m_pkgselect_work[i + 1];
		}
		m_pkgselect_work[s_titleNum - 1] = l;
	}
	function move_r( )
	{
		local r = m_pkgselect_work[s_titleNum - 1];

    local i = 0;
    for ( i = s_titleNum - 1; i >= 1 ; i-- )
    {
			m_pkgselect_work[i] = m_pkgselect_work[i-1];
		}
		m_pkgselect_work[0] = r;
	}


	function exec( )
	{
		if( m_isExec == false )
		{
			if( m_move != 0 )
			{
				m_count--;
				if( m_count == 0 )
				{
					//ずらす
					if( m_move == -1 ) 
					{
						move_l( );
						m_index++;
						if(m_index >= s_titleNum )
						{
							m_index = 0;
						}
					}
					else
					{
						move_r( );
						m_index--;
						if(m_index < 0 )
						{
							m_index = s_titleNum - 1;
						}
					}
					m_move = 0;
	/*
					local i = 0;
			    for ( i = 0; i < s_titleNum; i++ )
			    {
						printf( "m_pkgselect_work[%d] = %d\n",i, m_pkgselect_work[i] );
					}

					printf( "m_index = %d\n",m_index );
	*/
					nameTextUpdate();

					selectPkgUpdate();
				}
			}
		}
		
		local offset_x = 0;
		if( m_move == -1 ) 
		{
			offset_x = -( SLIDEWAIT - m_count ) * ( PKG_W / SLIDEWAIT );
		}
		else if ( m_move == 1 )
		{
			offset_x = ( SLIDEWAIT - m_count ) * ( PKG_W / SLIDEWAIT );
		}
		
		//スクロールする選択パッケージ
		local i = 0;
    for ( i = 0; i < s_titleNum; i++ )
    {
			local frameoffset_x = 0;
			switch( i )
			{
			case 0:
				frameoffset_x = -2 * getFrameOffset( );
				break;
			case 1:
				frameoffset_x = -1 * getFrameOffset( );
				break;
			case 2:
				break;
			case 3:
				frameoffset_x = 1 * getFrameOffset( );
				break;
			case 4:
				frameoffset_x = 2 * getFrameOffset( );
				break;
			}

			m_pkgselect_motion[i].left = 0 + ( i * PKG_W ) + offset_x - ( PKG_W * CUNTERINDEX ) + frameoffset_x;	//0,0が画面中央
			m_pkgselect_motion[i].top = PKG_Y;

			if( m_isExec == true ) //ゲーム起動演出
			{
				//起動するパッケージを透明にする
				m_pkgselect_motion[i].opacity = m_pkgselect_motion[CUNTERINDEX].opacity - 8;
				if( m_pkgselect_motion[i].opacity < 0 )
				{
					m_pkgselect_motion[i].opacity = 0;
				}
				m_pkgselect_s_motion[i].opacity = m_pkgselect_motion[CUNTERINDEX].opacity - 8;
				if( m_pkgselect_s_motion[i].opacity < 0 )
				{
					m_pkgselect_s_motion[i].opacity = 0;
				}
			}
			else
			{
				//透明から表示する
				m_pkgselect_motion[i].opacity = m_pkgselect_motion[CUNTERINDEX].opacity + 8;
				if( m_pkgselect_motion[i].opacity > 255 )
				{
					m_pkgselect_motion[i].opacity = 255;
				}
			}

			if( m_isVisible == true )
			{
				if( i < 5 )
				{
					m_pkgselect_motion[i].visible = m_isVisible;
				}
				else
				{
					m_pkgselect_motion[i].visible = false;
				}
				m_pkgselect_s_motion[i].visible = m_isVisible;
			}
			else
			{
				m_pkgselect_motion[i].visible = false;
				m_pkgselect_s_motion[i].visible = false;
			}
			m_pkgselect_s_motion[i].top = THUMBNAIL_Y - getFrameOffset( );
		}

		//サムネイルのカーソル
		m_thumbnail_cursol_motion.left = ( THUMBNAIL_W * m_index ) - ( ( s_titleNum / 2 ) * THUMBNAIL_W );	//0,0が画面中央
		m_thumbnail_cursol_motion.top = THUMBNAIL_Y + 22 - getFrameOffset( );
	
		//テキストの位置
		m_text_Title.setCoord( 0, ( PKGNAME_TEXTOFFSET_Y - getFont32Yoffset() ) + getFrameOffset( ) );

		
		if( m_isVisible == true )
		{
			if( m_isCursolVisible == true )
			{
				m_text_Title.visible = m_isVisible;
				m_cursol_motion.visible = m_isVisible;
				m_thumbnail_cursol_motion.visible = m_isVisible;

				m_cursol_motion.opacity = m_cursol_motion.opacity + 8;
				if( m_cursol_motion.opacity > 255 )
				{
					m_cursol_motion.opacity = 255;
				}
				m_thumbnail_cursol_motion.opacity = m_thumbnail_cursol_motion.opacity + 8;
				if( m_thumbnail_cursol_motion.opacity > 255 )
				{
					m_thumbnail_cursol_motion.opacity = 255;
				}
			}
			else
			{
				m_text_Title.visible = false;
				m_cursol_motion.visible = false;
				m_thumbnail_cursol_motion.visible = false;
				m_motion_bg.setVariable("disable", 1);
				m_motion_bg_down.setVariable("disable", 1);
				m_cursol_motion.opacity = 0;
				m_thumbnail_cursol_motion.opacity = 0;
			}	
		}
		else
		{
			m_text_Title.visible = false;
			m_cursol_motion.visible = false;
				m_thumbnail_cursol_motion.visible = false;
			m_motion_bg.setVariable("disable", 1);
			m_motion_bg_down.setVariable("disable", 1);
			m_cursol_motion.opacity = 0;
			m_thumbnail_cursol_motion.opacity = 0;
		}

	}
	
	function setImageID( sort )
	{
		m_sort = sort;
		indexTableUpdate();
		selectPkgUpdate();

	}
  function setVisible( flg )
  {
		m_isVisible = flg;
	}
	// FOLDER HACK: synchronous fade-out before a folder swap. Decrements
	// per-package and per-thumbnail opacity over `frames` frames; caller
	// is expected to immediately destroy/rebuild m_selectPkg afterwards
	// (the rebuild's fadeIn=true then animates the new pack up from 0).
	// Skip if motions aren't allocated yet (defensive). Blocking — yields
	// each frame via wait(0).
	function fadeOut( frames = 12 )
	{
		if (m_pkgselect_motion == null) return;
		local n = m_pkgselect_motion.len();
		for (local f = 1; f <= frames; f++) {
			local o = 255 - (255 * f / frames);
			if (o < 0) o = 0;
			for (local i = 0; i < n; i++) {
				if (m_pkgselect_motion[i] != null) m_pkgselect_motion[i].opacity = o;
				if (m_pkgselect_s_motion != null && i < m_pkgselect_s_motion.len()
				    && m_pkgselect_s_motion[i] != null) {
					m_pkgselect_s_motion[i].opacity = o;
				}
			}
			if (m_cursol_motion != null) m_cursol_motion.opacity = o;
			if (m_thumbnail_cursol_motion != null) m_thumbnail_cursol_motion.opacity = o;
			wait(0);
		}
	}
  function setCursolVisible( flg ) 
  {
		m_isCursolVisible = flg;
	}
	
	function setIndex( idx )
	{

		m_index = idx;
		
    local i = 0;
    for ( i = 0; i < s_titleNum; i++ )
    {
			m_pkgselect_work[i] = i;
		}
		for ( i = 0; i < CUNTERINDEX; i++ )
		{
			move_r( )
		}
		for ( i = 0; i < idx; i++ )
		{
			move_l( )
		}
/*
		printf( "setIndex start\n" );
    for ( i = 0; i < s_titleNum; i++ )
    {
			printf( "m_pkgselect_work[%d] = %d\n",i, m_pkgselect_work[i] );
		}
		printf( "m_index = %d\n",m_index );
*/
		nameTextUpdate();
		selectPkgUpdate();
	}

	function selectPkgUpdate()
	{
		local i = 0;
    for ( i = 0; i < s_titleNum; i++ )
    {
			local selectindex = m_pkgselect_work[i];
			local image = getLinenpOffsetConfigData(selectindex, "image");
			local ccolor = getLinenpOffsetConfigData(selectindex, "ccolor");
			local csize = getLinenpOffsetConfigData(selectindex, "csize");

			switch( ccolor )
			{
			case 0:
			case 5:
				//ケース黒
				m_pkgselect_motion[i].chara   = "common_parts";
				m_pkgselect_motion[i].motion  = "pkg";
				break;
			case 1:
				//ケース白
				m_pkgselect_motion[i].chara   = "common_parts";
				m_pkgselect_motion[i].motion  = "pkg_white";
				break;
			case 2:
				//2枚組ディスク
				m_pkgselect_motion[i].chara   = "common_parts";
				m_pkgselect_motion[i].motion  = "pkg_wide";
				break;
			case 3:
				//SGケース
				m_pkgselect_motion[i].chara   = "common_parts";
				m_pkgselect_motion[i].motion  = "pkg_sg";
				break;
			}
			
			if(selectindex < LINEUPMAX)
			{
				m_pkgselect_motion[i].setVariable("pkg", image);
			}
			else
			{
				m_pkgselect_motion[i].setVariable("pkg", 29);
			}

			if( DEMOVERSISON == 1 )
			{
				//デモバージョンではすべてHuカードにする
				local sordemo = getLinenpOffsetConfigData(selectindex, "sor_demo");
				if( sordemo > 2 )
				{
					//ソート３以降は選択できない
					m_pkgselect_motion[i].chara   = "common_parts";
					m_pkgselect_motion[i].motion  = "pkg";
					m_pkgselect_motion[i].setVariable("pkg", 31);
				}
			}
		}
	}
	function nameTextUpdate()
	{

		//タイトル設定
		local selectindex = m_pkgselect_work[CUNTERINDEX];
		local indexoffset = getLanguageIndexOffset( );	//起動するリージョンによってオフセットを加える
		local sortindex = s_indextable[selectindex] + indexoffset;
		local tname = m_config["items"][sortindex]["tname"];

		if( m_isVisible == true )
		{
			if( m_isCursolVisible == true )
			{
				m_text_Title.visible = m_isVisible;
			}
			else
			{
				m_text_Title.visible = false;
			}
		}
		else
		{
			m_text_Title.visible = false;
		}
		
		if(selectindex < LINEUPMAX)
		{
			m_text_Title.print("#{color,000000FF}"+tname);
		}
		else
		{
			m_text_Title.print("#{color,FF0000FF}"+"DUMMY");
		}

		IndicatorSetXScale( m_text_Title, INDEX_LENGTH_MAX );

		//タイトルバー
		local titlebar = getLinenpOffsetConfigData(selectindex, "titlebar");
		m_motion_bg.setVariable("titlebar", titlebar);
		m_motion_bg_down.setVariable("titlebar", titlebar);
//		printf("titlebar = %d\n", titlebar);

		//プレイ人数
		local players = getLinenpOffsetConfigData(selectindex, "players");
		m_motion_bg.setVariable("playernum", players);
		m_motion_bg_down.setVariable("playernum", players);

		
		//セーブスロット
//		local backupStatedata = BackupStatedata(null, false); // エラー表示なし

		local sortindexLineup = s_indextable[selectindex] + getLinenpIndexOffset( );
		local i = 0;
		for ( i = 0; i < 4; i++ )
		{
			local exist = 0;
//			if ( backupStatedata.check_exist_state_data(i + ( id * 4 )) == true ) 

//			local fileid = ( i + ( ( id + indexoffset ) * 4 ) );
			local fileid = ( i + ( ( sortindexLineup ) * 4 ) );
			if ( ::g_frameCount.stateFileExist( fileid ) == true ) 
			{
				exist = 1;
			}
			switch( i )
			{
			case 0:
				m_motion_bg.setVariable("saveslot1", exist);
				m_motion_bg_down.setVariable("saveslot1", exist);
				break;
			case 1:
				m_motion_bg.setVariable("saveslot2", exist);
				m_motion_bg_down.setVariable("saveslot2", exist);
				break;
			case 2:
				m_motion_bg.setVariable("saveslot3", exist);
				m_motion_bg_down.setVariable("saveslot3", exist);
				break;
			case 3:
				m_motion_bg.setVariable("saveslot4", exist);
				m_motion_bg_down.setVariable("saveslot4", exist);
				break;
			}
		}

		//カーソル
		//ＳＧは専用カーソルにする
		local csize = getLinenpOffsetConfigData(selectindex, "csize");
		if( csize == 1 )
		{
			m_cursol_motion.setVariable("cursol_size", 1);
		}
		else
		{
			m_cursol_motion.setVariable("cursol_size", 0);
		}
		
		if( DEMOVERSISON == 1 )
		{
			//デモバージョンではすべてHuカードにする
			local sordemo = getLinenpOffsetConfigData(selectindex, "sor_demo");
			if( sordemo > 2 )
			{
				m_text_Title.print("#{color,000000FF}"+"???");
				m_motion_bg.setVariable("titlebar", 0);
				m_motion_bg_down.setVariable("titlebar", 0);
		//		printf("titlebar = %d\n", titlebar);

				//プレイ人数
				local players = getLinenpOffsetConfigData(selectindex, "players");
				m_motion_bg.setVariable("playernum", 0);
				m_motion_bg_down.setVariable("playernum", 0);
			}
		}
		
	}
	function indexTableUpdate()
	{
		local i = 0;
		for ( i = 0; i < s_titleNum; i++ )
		{
			s_indextable[i] = LINEUPMAX;
		}
		for ( i = 0; i < s_titleNum; i++ )
		{
			local index = getPkgIndex( i, m_sort, m_config );
			s_indextable[index] = i;
		}
		for ( i = 0; i < s_titleNum; i++ )
		{
			local selectindex = i;
			local image = getLinenpOffsetConfigData( selectindex, "image" );
			m_pkgselect_s_motion[i].setVariable("pkg", image);
			if( DEMOVERSISON == 1 )
			{
				//デモバージョンではすべてHuカードにする
				local sordemo = getLinenpOffsetConfigData(selectindex, "sor_demo");
				if( sordemo > 2 )
				{
					m_pkgselect_s_motion[i].setVariable("pkg", 31);
				}
			}

//			printf( "s_indextable[%d] = %d\n",i, s_indextable[i] );
		}
	}
	
	function getIndex()
	{
		return( m_index );
	}

	function setLineup( lineup )
	{
		m_linenp = lineup;
	}

	function gameStart( isSelectRun = false )
	{
		m_isExec = true;
		
		//枠を消す
		m_motion_bg.setVariable("disable", 1);
		m_motion_bg.setVariable("playernum", 0);
		m_motion_bg_down.setVariable("disable", 1);
		m_motion_bg_down.setVariable("playernum", 0);

		//カーソルを消す
		setCursolVisible( false );

		local selectindex = m_pkgselect_work[CUNTERINDEX];
		local image = getLinenpOffsetConfigData( selectindex, "image" );
		local csize = getLinenpOffsetConfigData( selectindex, "csize" );
		m_exec_motion = Motion(m_layer);
		m_exec_motion.chara   = getGameStartBGMotionName();
		switch( csize )
		{
		case 0:
		case 5:
			m_exec_motion.motion  = "hucard";
			break;
		case 1:
			m_exec_motion.motion  = "boot_sg";
			break;
		case 2:
			m_exec_motion.motion  = "boot_cdromrom";
			break;
		case 3:
			if( isSelectRun == true )
			{
				m_exec_motion.motion  = "boot_cdromrom";
			}
			else
			{
				m_exec_motion.motion  = "boot_super";
			}
			break;
		case 4:
			if( isSelectRun == true )
			{
				m_exec_motion.motion  = "boot_cdromrom";
			}
			else
			{
				m_exec_motion.motion  = "boot_arcade";
			}
			break;
		}
		m_exec_motion.opacity = 255;
		m_exec_motion.independentLayerInherit = true;
		m_exec_motion.progress();
		m_exec_motion.left = 0;	//0,0が画面中央
		m_exec_motion.top = 0;

		m_exec_motion.setVariable("lineup", m_linenp);

		if(selectindex < LINEUPMAX)
		{
			m_pkgselect_motion[CUNTERINDEX].setVariable("pkg", image);
			m_motion_bg.setVariable("pkg", image);
			m_motion_bg_down.setVariable("pkg", image);
			m_exec_motion.setVariable("pkg", image);
		}
		m_exec_motion.visible = true;
	}

}


class LinenpChange
{
	m_layer = null;
	m_motion = null;
	m_isIn = null;

  constructor( isIn ) 
  {
		m_isIn = isIn;

		m_layer = ScaledLayer();
		m_layer.visible = true;
		m_layer.smoothing = MOTSMOOTHING;
		m_layer.priority = PRI_MASK;
		m_layer.registerMotionResource(s_rsc.find(s_ui_motionPath));  // レイヤにモーションリソースを登録
		
		_init();
	}
	function _init()
	{
		m_motion = Motion(m_layer);
		m_motion.chara   = "frame";
		if( m_isIn )
		{
			m_motion.motion  = "change_mask_in";
		}
		else
		{
			m_motion.motion  = "change_mask_out";
		}
		m_motion.opacity = 255;
		m_motion.visible = true;
		m_motion.independentLayerInherit = true;
		m_motion.progress();
		m_motion.left = 0;	//0,0が画面中央
		m_motion.top = 0;
	}
}
