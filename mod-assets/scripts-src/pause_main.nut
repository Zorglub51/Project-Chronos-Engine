//----------------------------------------------------------------------
// pasue（メイン）
//----------------------------------------------------------------------

const DEBUGDISP = 0;

//プライオリティ
const PRI_BG = 30;
const PRI_TEXT = 32;
const PRI_PKG = 35;
const PRI_CSL = 41;
const PRI_STIMAGE = 37;
const PRI_STTEXT = 38;
const PRI_TITLEBAR = 39;
const PRI_TITLEBAR_NAME = 40;
const PRI_ARROW = 41;
const PRI_DEBUG = 42;

s_pause_motionPath <-  null;

const OPENANIME = 15;

function pause_main(_session, _play_replay = false, _replay_mode = BackupGameMode.NORMAL, _disable_state_save = false, _title_control = null, _pause_pad_id = null)
{
//	::util_load_script("system/script/mode_title_select.nut");

	::g_frameCount.logFileWrite("pause_main start");

	if( s_pause_motionPath == null )
	{
		s_pause_motionPath = conv_path(MENU_UI_TITLESELECT_MOTION_PATH);
	//	s_pause_motionPath = conv_path(MENU_UI_TITLESELECT_PAUSE_MOTION_PATH);
	}

	::g_frameCount.logFileWrite("pause_main 1");

  local menu = MenuPauseMain(_session, _play_replay, _replay_mode, _disable_state_save, _title_control, _pause_pad_id);

	::g_frameCount.logFileWrite("pause_main end");

  return menu.exec();
}

// --
const MENU_SAVE = 0;
const MENU_LOAD = 1;
//const MENU_RESET = 2;
const MENU_RETURNMENU = 2;
const MENU_RESUME = 3;
const MENU_MAX = 4;

const SEQ_OPEN = 0;
const SEQ_MAIN = 1;
const SEQ_CLOSE = 2;

const DATETXTOFS_Y = 60;

const STIMAGEOFS_X = 80;
const STIMAGEOFS_Y = -64;

class MenuPauseBase {

	m_session						 = null;
  m_play_replay        = null;
  m_replay_mode        = null;
  m_disable_state_save = null;
  m_title_control      = null;
  m_pause_pad_id       = null;

  m_child_result       = null;
	m_cold_reset         = null;		//ゲームをリセットする
	m_loop_exit          = null;		//ポーズ終了
	m_current_language   = null;
	m_current_lists_index   = null;		//現在選択しているロムテーブル

	m_motion_cur = null;
	m_index = null;
	m_keywait = null;

  m_layer = null;
	m_motion_bg = null;		//背景

	m_motion_savecur = null;
	m_motion_StateDisp = null;
	m_saveindex = null;

	m_mode = null;
	m_lockData = null;

	m_seq = null;
	m_wait = null;

	m_file_lists	= null;
	
	m_debug_text = null;

  m_layerText = null;
	m_config = null;
	m_caption = null;
	m_guide = null;
	m_savetext = null;
	m_text = null;

	m_titletest_count = null;

  constructor(_session, _play_replay = false, _replay_mode = BackupGameMode.NORMAL, _disable_state_save = false, _title_control = null, _pause_pad_id = null)
  {
		::g_frameCount.logFileWrite("pause_main constructor start");

	 	init();

		::g_frameCount.logFileWrite("pause_main constructor 1");

		m_session = _session;
    m_play_replay        = _play_replay;
    m_replay_mode        = _replay_mode;
    m_disable_state_save = _disable_state_save;
    m_title_control      = _title_control;
    m_pause_pad_id       = _pause_pad_id;

		m_child_result = false;	//ゲームを終了する
		m_cold_reset = false;		//ゲームをリセットする
		m_loop_exit = false;			//ポーズ終了

		::g_frameCount.logFileWrite("pause_main constructor 2");

		m_motion_cur = PauseCorsol();
		m_motion_cur.setVisible(false);
		m_index = MENU_SAVE;
		m_keywait = 0;

		m_seq = SEQ_OPEN;
		m_wait = 0;
		m_titletest_count = 0;

		::g_frameCount.logFileWrite("pause_main constructor 3");

		m_motion_savecur = PauseCorsol();
		m_motion_savecur.setSaveCorsol( );
		m_motion_savecur.setVisible(false);
		
		m_mode = 0;

		::g_frameCount.logFileWrite("pause_main constructor 4");

		m_lockData = [];
		m_lockData.append(false);
		m_lockData.append(false);
		m_lockData.append(false);
		m_lockData.append(false);
		//セーブデータからロック情報を入れる

		::g_frameCount.logFileWrite("pause_main constructor 5");

		local slock = 0;
		// V2 locks are keyed by game identity, never by the current menu position.
		if (!::chronos_save_enabled())
		{
		switch( m_current_lists_index )
		{
		case 0:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_10_lockdata00();
			break;
		case 1:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_11_lockdata01();
			break;
		case 2:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_12_lockdata02();
			break;
		case 3:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_13_lockdata03();
			break;
		case 4:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_14_lockdata04();
			break;
		case 5:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_15_lockdata05();
			break;
		case 6:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_16_lockdata06();
			break;
		case 7:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_17_lockdata07();
			break;
		case 8:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_18_lockdata08();
			break;
		case 9:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_19_lockdata09();
			break;
		case 10:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_20_lockdata10();
			break;
		case 11:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_21_lockdata11();
			break;
		case 12:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_22_lockdata12();
			break;
		case 13:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_23_lockdata13();
			break;
		case 14:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_24_lockdata14();
			break;
		case 15:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_25_lockdata15();
			break;
		case 16:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_26_lockdata16();
			break;
		case 17:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_27_lockdata17();
			break;
		case 18:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_28_lockdata18();
			break;
		case 19:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_29_lockdata19();
			break;
		case 20:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_30_lockdata20();
			break;
		case 21:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_31_lockdata21();
			break;
		case 22:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_32_lockdata22();
			break;
		case 23:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_33_lockdata23();
			break;
		case 24:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_34_lockdata24();
			break;
		case 25:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_35_lockdata25();
			break;
		case 26:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_36_lockdata26();
			break;
		case 27:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_37_lockdata27();
			break;
		case 28:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_38_lockdata28();
			break;
		case 29:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_39_lockdata29();
			break;
		case 30:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_40_lockdata30();
			break;
		case 31:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_41_lockdata31();
			break;
		case 32:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_42_lockdata32();
			break;
		case 33:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_43_lockdata33();
			break;
		case 34:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_44_lockdata34();
			break;
		case 35:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_45_lockdata35();
			break;
		case 36:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_46_lockdata36();
			break;
		case 37:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_47_lockdata37();
			break;
		case 38:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_48_lockdata38();
			break;
		case 39:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_49_lockdata39();
			break;
		case 40:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_50_lockdata40();
			break;
		case 41:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_51_lockdata41();
			break;
		case 42:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_52_lockdata42();
			break;
		case 43:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_53_lockdata43();
			break;
		case 44:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_54_lockdata44();
			break;
		case 45:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_55_lockdata45();
			break;
		case 46:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_56_lockdata46();
			break;
		case 47:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_57_lockdata47();
			break;
		case 48:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_58_lockdata48();
			break;
		case 49:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_59_lockdata49();
			break;
		case 50:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_60_lockdata50();
			break;
		case 51:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_61_lockdata51();
			break;
		case 52:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_62_lockdata52();
			break;
		case 53:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_63_lockdata53();
			break;
		case 54:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_64_lockdata54();
			break;
		case 55:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_65_lockdata55();
			break;
		case 56:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_66_lockdata56();
			break;
		case 57:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_67_lockdata57();
			break;
		case 58:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_68_lockdata58();
			break;
		case 59:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_69_lockdata59();
			break;

		case 60:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_100_lockdata60();
			break;
		case 61:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_101_lockdata61();
			break;
		case 62:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_102_lockdata62();
			break;
		case 63:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_103_lockdata63();
			break;
		case 64:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_104_lockdata64();
			break;
		case 65:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_105_lockdata65();
			break;
		case 66:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_106_lockdata66();
			break;
		case 67:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_107_lockdata67();
			break;
		case 68:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_108_lockdata68();
			break;
		case 69:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_109_lockdata69();
			break;
		case 70:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_110_lockdata70();
			break;
		case 71:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_111_lockdata71();
			break;
		case 72:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_112_lockdata72();
			break;
		case 73:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_113_lockdata73();
			break;
		case 74:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_114_lockdata74();
			break;
		case 75:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_115_lockdata75();
			break;
		case 76:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_116_lockdata76();
			break;
		case 77:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_117_lockdata77();
			break;
		case 78:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_118_lockdata78();
			break;
		case 79:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_119_lockdata79();
			break;
		case 80:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_120_lockdata80();
			break;
		case 81:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_121_lockdata81();
			break;
		case 82:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_122_lockdata82();
			break;
		case 83:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_123_lockdata83();
			break;
		case 84:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_124_lockdata84();
			break;
		case 85:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_125_lockdata85();
			break;
		case 86:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_126_lockdata86();
			break;
		case 87:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_127_lockdata87();
			break;
		case 88:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_128_lockdata88();
			break;
		case 89:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_129_lockdata89();
			break;
		case 90:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_130_lockdata90();
			break;
		case 91:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_131_lockdata91();
			break;
		case 92:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_132_lockdata92();
			break;
		case 93:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_133_lockdata93();
			break;
		case 94:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_134_lockdata94();
			break;
		case 95:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_135_lockdata95();
			break;
		case 96:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_136_lockdata96();
			break;
		case 97:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_137_lockdata97();
			break;
		case 98:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_138_lockdata98();
			break;
		case 99:
			slock = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_139_lockdata99();
			break;

		}
		}
		slock = ::chronos_get_save_locks(slock);

		if( slock & 0x0001 )
		{
			m_lockData[0] = true;
		}
		if( slock & 0x0002 )
		{
			m_lockData[1] = true;
		}
		if( slock & 0x0004 )
		{
			m_lockData[2] = true;
		}
		if( slock & 0x0008 )
		{
			m_lockData[3] = true;
		}

		::g_frameCount.logFileWrite("pause_main constructor 6");

		//ステート表示		
		m_motion_StateDisp = [];
		m_motion_StateDisp.append(StateDisp());
		m_motion_StateDisp.append(StateDisp());
		m_motion_StateDisp.append(StateDisp());
		m_motion_StateDisp.append(StateDisp());

		m_motion_StateDisp[0].setPosition(-390,62);
		m_motion_StateDisp[1].setPosition(-130,62);
		m_motion_StateDisp[2].setPosition( 130,62);
		m_motion_StateDisp[3].setPosition( 390,62);
		
		local i = 0;
		for ( i = 0; i < 4; i++ )
		{
			m_motion_StateDisp[i].setLock(m_lockData[i]);
			m_motion_StateDisp[i].setFileData( m_file_lists[i],  true );
			m_file_lists[i].thmubnail = null;
		}
		stateVisible( false );

		::g_frameCount.logFileWrite("pause_main constructor 7");


		if( DEBUGDISP == 1 )
		{
			m_debug_text = [];
	    local font_path = conv_path(MENU_MULTI_FONT_PATH);

			for ( i = 0; i < 20; i++ )
			{
				local _str = "1";
				switch( i )
				{
				case 0:
					_str = "1";
					break;
				case 1:
					_str = "2";
					break;
				case 2:
					_str = "3";
					break;
				case 3:
					_str = "4";
					break;
				case 4:
					_str = "5";
					break;
				case 5:
					_str = "6";
					break;
				case 6:
					_str = "7";
					break;
				case 7:
					_str = "8";
					break;
				case 8:
					_str = "9";
					break;
				case 9:
					_str = "10";
					break;
				case 10:
					_str = "11";
					break;
				}
				m_debug_text.append( Indicator(m_layer, s_rsc.find(font_path)) );
				m_debug_text[i].visible = true;
				m_debug_text[i].setRecognizeTag(true);
				m_debug_text[i].fontColor = TEXT_COLOR_NORMAL;
				m_debug_text[i].setAlignment(CONSOLE.ALIGNMENT_CENTER);
				m_debug_text[i].print(_str);
				m_debug_text[i].setCoord( i * 30, 200 );
			}
		}
		::g_frameCount.logFileWrite("pause_main constructor end");

	}
	
  function init()
  {
//		::g_menu_sound.on_enter(); // SE再生
		::g_menu_sound.on_pause(); // SE再生

		::g_frameCount.logFileWrite("pause_main init start");

    m_config = ::util_load_config( ::conv_path("config/pause_main_item.psb") );

    ::g_emu_task.updateEmuScreen(); // XXX これが有効なタイトルがスクリーン設定で1フレーム進んでしまうのを予め進めることで回避(ただしリプレイ一時停止中ポーズから開くと1進んでしまう)
    ::create_savedata_capture(); // セーブデータicon用 emu画面キャプチャ


    ::g_systemdata.capture_system_all(); // システム状態をsystemdataに取得
    ::g_systemdata.CheckValueChangeAutosave(SystemDataValueIndex.WORK_MEDAL); // ゲーム中に変わる可能性がある実績用ワークが変化していたらAUTOセーブ
//    ::g_systemdata.TryAutosave(); // AUTOセーブ（これまでのAUTOセーブ要求はクリアされる）

		m_current_language = getlastLanguage();	//使用言語
		m_current_lists_index = getlastIndex() + getLinenpIndexOffset( );	//テーブルindex

		//最後にアクセスしたステート番号
//		m_saveindex = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_last_file_id_state() % 4;
		m_saveindex = ::g_saveIndex;

		_state_update();
		
    m_layerText = ScaledLayer(::util_get_menu_layerFolder());
    m_layerText.visible = true;
    m_layerText.smoothing = true;
		m_layerText.priority = PRI_TEXT;

    m_layer = ScaledLayer(::util_get_menu_layerFolder());
    m_layer.visible = true;
    m_layer.smoothing = MOTSMOOTHING;
		m_layer.priority = PRI_BG;

//    local font_path1 = getTitleMultiFontPath( );
//    local font_path2 = getMultiFontPath();
    local motion_path = conv_path(s_pause_motionPath);

    m_layer.registerMotionResource(s_rsc.find(motion_path));  // レイヤにモーションリソースを登録

		//BG作成
		m_motion_bg         = Motion(m_layer);
		m_motion_bg.chara   = "pause";
		m_motion_bg.motion  = "bg_open";
		m_motion_bg.opacity = 255;
		m_motion_bg.visible = true;
		m_motion_bg.independentLayerInherit = true;
		m_motion_bg.progress();

		m_motion_bg.setVariable("lang", m_current_language);
//		printf("m_current_language=%d\n", m_current_language );

		createText( 0, false );
		textVisible( false );

		::g_frameCount.logFileWrite("pause_main init end");
  }

	function exec()
	{
		::g_frameCount.logFileWrite("MenuPauseMain exec start");

		local input       = ::g_input;

		local child_result = false;	//メニューに戻る
		local cold_reset = false;		//リセットする
		while (!m_loop_exit)
		{

			if( DEBUGDISP == 1 )
			{
				local i;
				for ( i = 0; i < 20; i++ )
				{
					m_debug_text[i].visible = false;
				}
				if ( ::g_input.key(KEY.A) ) 
				{
					m_debug_text[0].visible = true;
				}
				if ( ::g_input.key(KEY.B) ) 
				{
					m_debug_text[1].visible = true;
				}
				if ( ::g_input.key(KEY.C) ) 
				{
					m_debug_text[2].visible = true;
				}
				if ( ::g_input.key(KEY.X) ) 
				{
					m_debug_text[3].visible = true;
				}
				if ( ::g_input.key(KEY.Y) ) 
				{
					m_debug_text[4].visible = true;
				}
				if ( ::g_input.key(KEY.Z) ) 
				{
					m_debug_text[5].visible = true;
				}
				if ( ::g_input.key(KEY.START) ) 
				{
					m_debug_text[6].visible = true;
				}
			}

/*
			if ( ::g_input.keyPressed(KEY.A) ) 
			{
				_resume();
			}
			if ( ::g_input.keyPressed(KEY.B) ) 
			{
			}
			if ( ::g_input.keyPressed(KEY.X) ) 
			{
			}
*/

			switch ( m_seq )
			{
			case SEQ_OPEN:
				//出現アニメ
				m_wait++;
				if( m_wait > OPENANIME )
				{
					m_wait = 0;
					m_seq = SEQ_MAIN;
					m_motion_bg.motion  = "bg";
					m_motion_cur.setVisible(true);
					stateVisible( true );
					textVisible( true );

					::g_emu_task.visible = false;
				}
				break;
			case 1:
				if( TITLESELECTTEST != 0 )
				{
					//デバッグタイトル連続選択
					m_titletest_count++;	

					if ( g_pauseCount == 0 )
					{
						if ( m_titletest_count == 30 ) 
						{
							::g_menu_sound.on_enter(); // SE再生
							for ( m_saveindex = 0; m_saveindex < 4; m_saveindex++ )	//4スロットセーブしてみる
							{
								local fileid = m_saveindex + (m_current_lists_index * 4);	//ファイルID
								//ステートセーブ
								local res = 1;
								if ( !this._check_input_quick_state_saveload("save") ) {
								  ::set_list_control_pad_id(m_pause_pad_id);
								  res = ::save_state_data(fileid);
								}

								if (res) {
								  ::g_systemdata.CheckValueChangeAutosave(SystemDataValueIndex.BACKUP_FLAGS); // 中断データカーソル位置更新チェック
	//										  ::g_systemdata.TryAutosave(); // AUTOセーブを試みる（これまでのAUTOセーブ要求はクリアされる）
								  m_child_result = m_session.wait_check();
								  this.after_StateSave();

									//一度でもステートを保存したら初期化ファイルを作成する
									::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_73_stateSavet(1);
									//本来はオートセーブで作る方が処理が統一されるのでいいのだけども
									//レスポンスが悪いので直接作る事にする
									//セーブデータを更新してあるので、次回の保存でファイルはつくられるはず
									::g_frameCount.initfileSave();	//初期化確認用ファイル作成
	//											::g_systemdata.TryAutosave(); // AUTOセーブを試みる（これまでのAUTOセーブ要求はクリアされる）

								}
							}
							m_saveindex = 0;
							::g_saveIndex = m_saveindex;	//セーブカーソル位置の保存
						}
						if ( m_titletest_count == 60 ) 
						{
							local fileid = m_saveindex + (m_current_lists_index * 4);	//ファイルID
							::g_menu_sound.on_enter(); // SE再生
							::g_saveIndex = m_saveindex;	//セーブカーソル位置の保存

							local res = 1;
							if ( !this._check_input_quick_state_saveload("load") ) {
							  ::set_list_control_pad_id(m_pause_pad_id);
							  res = ::load_state_data(fileid);
							}
							::release_savedata_capture(); // セーブデータicon用 emu画面キャプチャ解放
							wait(0);
							::create_savedata_capture();  // セーブデータicon用 emu画面キャプチャ

							if (res) {
							  if (null != m_title_control) {
							    m_title_control.sys_initialize(m_play_replay, ::g_emu_task, ::g_systemdata, true);
							  }
							  m_child_result = m_session.wait_check();

							  m_disable_state_save = false; // ロードできたなら、必ずセーブできる場所でもある
							  this.after_StateLoad();
								m_seq = SEQ_CLOSE;
							}
						}
					}
					else
					{
						if ( m_titletest_count == 30 ) 
						{
							::g_menu_sound.on_enter(); // SE再生
							_return_to();
						}
					}
				}

				//カーソル移動
				m_keywait--;
				if( m_keywait < 0 )
				{
					m_keywait = 0;
				}

				if( m_mode == 0 )
				{
					if ( m_keywait == 0 )
					{
						if ( ::g_input.key(KEY.UP) )
						{
							::g_menu_sound.on_move_cursor(); // SE再生
							m_index--;
							if( m_index < 0 )
							{
								m_index = MENU_MAX - 1;
							}
							m_keywait = KEYWAIT;		//キー入力ウエイト
						}
						if ( ::g_input.key(KEY.DOWN) ) 
						{
							::g_menu_sound.on_move_cursor(); // SE再生
							m_index++;
							if( m_index >= MENU_MAX )
							{
								m_index = 0;
							}
							m_keywait = KEYWAIT;		//キー入力ウエイト
						}
						if( m_index == MENU_MAX - 1)
						{
								m_motion_cur.setScreenCorsol( 2 );
						}
						else
						{
								m_motion_cur.setScreenCorsol( 0 );
						}
					}
					if ( m_keywait == 0 )
					{
						if( ::g_input.keyPressed(KEY.A) )
						{
							switch (m_index)
							{
							case MENU_SAVE:
								if ( DEMOVERSISON == 0 )
								{
									::g_menu_sound.on_enter(); // SE再生
									m_mode = 1;
									m_motion_savecur.setVisible(true);
									createText(1);
								}
								else
								{
									::g_menu_sound.on_back(); // SE再生
								}
								break;
							case MENU_LOAD:
								if ( DEMOVERSISON == 0 )
								{
									::g_menu_sound.on_enter(); // SE再生
									m_mode = 2;
									m_motion_savecur.setVisible(true);
									createText(1);
								}
								else
								{
									::g_menu_sound.on_back(); // SE再生
								}
								break;
/*
							case MENU_RESET:
								_reset_game();
								break;
*/
							case MENU_RETURNMENU:
								::g_menu_sound.on_enter(); // SE再生
								_return_to();
								break;
							case MENU_RESUME:
								::g_menu_sound.on_enter(); // SE再生
								_resume();
								break;
							}
						}
						else if ( ::g_input.keyPressed(KEY.B) ) 
						{
							::g_menu_sound.on_back(); // SE再生
							_resume();
						}
						else if ( ::g_input.keyPressed(KEY.SELECT) )
						{
							::g_menu_sound.on_enter(); // SE再生
							_reset_game();
						}
						
//						local paused_by_pad_key = input.keyPressed(::util_get_pad_key_for_menu()) | input.keyPressed(KEY.L2);
						local paused_by_pad_key = input.keyPressed(KEY.L2);
						if( paused_by_pad_key )
						{
							::g_menu_sound.on_enter(); // SE再生
							_resume();
						}
					}
				}
				else
				{
					if ( m_keywait == 0 )
					{
						if ( ::g_input.key(KEY.LEFT) ) 
						{
							::g_menu_sound.on_move_cursor(); // SE再生
							m_saveindex--;
							if( m_saveindex < 0 )
							{
								m_saveindex = 3;
							}
							m_keywait = KEYWAIT;		//キー入力ウエイト
						}
						if ( ::g_input.key(KEY.RIGHT) ) 
						{
							::g_menu_sound.on_move_cursor(); // SE再生
							m_saveindex++;
							if( m_saveindex >= 4 )
							{
								m_saveindex = 0;
							}
							m_keywait = KEYWAIT;		//キー入力ウエイト
						}
						if ( ::g_input.keyPressed(KEY.A) ) 
						{
							local fileid = m_saveindex + (m_current_lists_index * 4);	//ファイルID
							if ( m_mode == 1 )
							{
								//保存
								if( m_lockData[m_saveindex] == false )
								{
									//上書き可能
									::g_menu_sound.on_enter(); // SE再生
									
									local res = false;
									local msg = "";
									if( m_file_lists[m_saveindex].exist == false )
									{
										switch( m_saveindex )
										{
										case 0:
											msg = "NoticeMsg__SAVESTATE1";
											break;
										case 1:
											msg = "NoticeMsg__SAVESTATE2";
											break;
										case 2:
											msg = "NoticeMsg__SAVESTATE3";
											break;
										case 3:
											msg = "NoticeMsg__SAVESTATE4";
											break;
										}
									}
									else
									{
										switch( m_saveindex )
										{
										case 0:
											msg = "NoticeMsg__SAVESTATE1_OVERWRITE";
											break;
										case 1:
											msg = "NoticeMsg__SAVESTATE2_OVERWRITE";
											break;
										case 2:
											msg = "NoticeMsg__SAVESTATE3_OVERWRITE";
											break;
										case 3:
											msg = "NoticeMsg__SAVESTATE4_OVERWRITE";
											break;
										}
									}
									if( TITLESELECTTEST != 0 )
									{
										res = true;	//自動テストの場合は確認メッセージ出さない
									}
									else
									{
										res = ::confirm_dialog(msg);	//確認メッセージ
									}
									
									if( res == true )
									{
										//ステートセーブ
										::g_saveIndex = m_saveindex;	//セーブカーソル位置の保存
										local res = 1;
										if ( !this._check_input_quick_state_saveload("save") ) {
										  ::set_list_control_pad_id(m_pause_pad_id);
										  res = ::save_state_data(fileid);
										}

										if (res) {
										  ::g_systemdata.CheckValueChangeAutosave(SystemDataValueIndex.BACKUP_FLAGS); // 中断データカーソル位置更新チェック
//										  ::g_systemdata.TryAutosave(); // AUTOセーブを試みる（これまでのAUTOセーブ要求はクリアされる）
										  m_child_result = m_session.wait_check();
										  this.after_StateSave();

											//一度でもステートを保存したら初期化ファイルを作成する
											::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_73_stateSavet(1);
											//本来はオートセーブで作る方が処理が統一されるのでいいのだけども
											//レスポンスが悪いので直接作る事にする
											//セーブデータを更新してあるので、次回の保存でファイルはつくられるはず
											::g_frameCount.initfileSave();	//初期化確認用ファイル作成
//											::g_systemdata.TryAutosave(); // AUTOセーブを試みる（これまでのAUTOセーブ要求はクリアされる）

										}
										m_mode = 0;	//メニュー移動に戻る
										m_motion_savecur.setVisible(false);
										createText(0);
									}
								}
								else
								{
									//ロックされている
									::g_menu_sound.on_ng(); // SE再生
								}
							}
							else
							{
								//読み込み
								//読み込みにロックは関係しない

								if( m_file_lists[m_saveindex].exist == true )
								{
									::g_menu_sound.on_enter(); // SE再生
									local res2 = false;
									local msg = "";
									switch( m_saveindex )
									{
									case 0:
										msg = "NoticeMsg__LOADSTATE1";
										break;
									case 1:
										msg = "NoticeMsg__LOADSTATE2";
										break;
									case 2:
										msg = "NoticeMsg__LOADSTATE3";
										break;
									case 3:
										msg = "NoticeMsg__LOADSTATE4";
										break;
									}
									res2 = ::confirm_dialog(msg);

									if( res2 == true )
									{
										::g_saveIndex = m_saveindex;	//セーブカーソル位置の保存

										local res = 1;
										if ( !this._check_input_quick_state_saveload("load") ) {
										  ::set_list_control_pad_id(m_pause_pad_id);
										  res = ::load_state_data(fileid);
										}
										::release_savedata_capture(); // セーブデータicon用 emu画面キャプチャ解放
										wait(0);
										::create_savedata_capture();  // セーブデータicon用 emu画面キャプチャ

										if (res) {
										  if (null != m_title_control) {
										    m_title_control.sys_initialize(m_play_replay, ::g_emu_task, ::g_systemdata, true);
										  }
										  m_child_result = m_session.wait_check();

										  m_disable_state_save = false; // ロードできたなら、必ずセーブできる場所でもある
										  this.after_StateLoad();
											m_seq = SEQ_CLOSE;
										}
									}
								}
								else
								{
									//ファイルが無い
									::g_menu_sound.on_ng(); // SE再生
								}
							}
						}
						else if ( ::g_input.keyPressed(KEY.B) ) 
						{
							::g_menu_sound.on_back(); // SE再生
							m_mode = 0;	//メニュー移動に戻る
							m_motion_savecur.setVisible(false);
							createText(0);
						}
						else if ( ::g_input.keyPressed(KEY.START) | ::g_input.keyPressed(KEY.R3) ) 
						{
//							::g_menu_sound.on_enter(); // SE再生
							::g_menu_sound.on_move_cursor(); // SE再生
							//データのロック
							if( m_lockData[m_saveindex] == false )
							{
								m_lockData[m_saveindex] = true;
							}
							else
							{
								m_lockData[m_saveindex] = false;
							}
							m_motion_StateDisp[m_saveindex].setLock(m_lockData[m_saveindex]);
						}
					}
				}

				break;
			case SEQ_CLOSE:
				if( m_wait == 0 )
				{
					m_motion_bg.motion  = "bg_close";
					m_motion_cur.setVisible(false);
					m_motion_savecur.setVisible(false);
					stateVisible( false );
					textVisible( false );

					::g_emu_task.visible = true;

					local slock = 0;

					if( m_lockData[0] == true )
					{
						slock = slock | 0x0001;
					}
					if( m_lockData[1] == true )
					{
						slock = slock | 0x0002;
					}
					if( m_lockData[2] == true )
					{
						slock = slock | 0x0004;
					}
					if( m_lockData[3] == true )
					{
						slock = slock | 0x0008;
					}

					if (!::chronos_set_save_locks(slock))
					{
					switch( m_current_lists_index )
					{
					case 0:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_10_lockdata00(slock);
						break;
					case 1:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_11_lockdata01(slock);
						break;
					case 2:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_12_lockdata02(slock);
						break;
					case 3:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_13_lockdata03(slock);
						break;
					case 4:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_14_lockdata04(slock);
						break;
					case 5:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_15_lockdata05(slock);
						break;
					case 6:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_16_lockdata06(slock);
						break;
					case 7:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_17_lockdata07(slock);
						break;
					case 8:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_18_lockdata08(slock);
						break;
					case 9:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_19_lockdata09(slock);
						break;
					case 10:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_20_lockdata10(slock);
						break;
					case 11:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_21_lockdata11(slock);
						break;
					case 12:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_22_lockdata12(slock);
						break;
					case 13:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_23_lockdata13(slock);
						break;
					case 14:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_24_lockdata14(slock);
						break;
					case 15:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_25_lockdata15(slock);
						break;
					case 16:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_26_lockdata16(slock);
						break;
					case 17:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_27_lockdata17(slock);
						break;
					case 18:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_28_lockdata18(slock);
						break;
					case 19:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_29_lockdata19(slock);
						break;
					case 20:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_30_lockdata20(slock);
						break;
					case 21:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_31_lockdata21(slock);
						break;
					case 22:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_32_lockdata22(slock);
						break;
					case 23:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_33_lockdata23(slock);
						break;
					case 24:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_34_lockdata24(slock);
						break;
					case 25:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_35_lockdata25(slock);
						break;
					case 26:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_36_lockdata26(slock);
						break;
					case 27:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_37_lockdata27(slock);
						break;
					case 28:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_38_lockdata28(slock);
						break;
					case 29:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_39_lockdata29(slock);
						break;
					case 30:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_40_lockdata30(slock);
						break;
					case 31:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_41_lockdata31(slock);
						break;
					case 32:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_42_lockdata32(slock);
						break;
					case 33:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_43_lockdata33(slock);
						break;
					case 34:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_44_lockdata34(slock);
						break;
					case 35:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_45_lockdata35(slock);
						break;
					case 36:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_46_lockdata36(slock);
						break;
					case 37:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_47_lockdata37(slock);
						break;
					case 38:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_48_lockdata38(slock);
						break;
					case 39:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_49_lockdata39(slock);
						break;
					case 40:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_50_lockdata40(slock);
						break;
					case 41:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_51_lockdata41(slock);
						break;
					case 42:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_52_lockdata42(slock);
						break;
					case 43:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_53_lockdata43(slock);
						break;
					case 44:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_54_lockdata44(slock);
						break;
					case 45:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_55_lockdata45(slock);
						break;
					case 46:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_56_lockdata46(slock);
						break;
					case 47:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_57_lockdata47(slock);
						break;
					case 48:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_58_lockdata48(slock);
						break;
					case 49:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_59_lockdata49(slock);
						break;
					case 50:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_60_lockdata50(slock);
						break;
					case 51:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_61_lockdata51(slock);
						break;
					case 52:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_62_lockdata52(slock);
						break;
					case 53:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_63_lockdata53(slock);
						break;
					case 54:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_64_lockdata54(slock);
						break;
					case 55:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_65_lockdata55(slock);
						break;
					case 56:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_66_lockdata56(slock);
						break;
					case 57:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_67_lockdata57(slock);
						break;
					case 58:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_68_lockdata58(slock);
						break;
					case 59:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_69_lockdata59(slock);
						break;


					case 60:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_100_lockdata60(slock);
						break;
					case 61:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_101_lockdata61(slock);
						break;
					case 62:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_102_lockdata62(slock);
						break;
					case 63:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_103_lockdata63(slock);
						break;
					case 64:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_104_lockdata64(slock);
						break;
					case 65:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_105_lockdata65(slock);
						break;
					case 66:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_106_lockdata66(slock);
						break;
					case 67:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_107_lockdata67(slock);
						break;
					case 68:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_108_lockdata68(slock);
						break;
					case 69:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_109_lockdata69(slock);
						break;
					case 70:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_110_lockdata70(slock);
						break;
					case 71:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_111_lockdata71(slock);
						break;
					case 72:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_112_lockdata72(slock);
						break;
					case 73:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_113_lockdata73(slock);
						break;
					case 74:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_114_lockdata74(slock);
						break;
					case 75:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_115_lockdata75(slock);
						break;
					case 76:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_116_lockdata76(slock);
						break;
					case 77:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_117_lockdata77(slock);
						break;
					case 78:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_118_lockdata78(slock);
						break;
					case 79:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_119_lockdata79(slock);
						break;
					case 80:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_120_lockdata80(slock);
						break;
					case 81:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_121_lockdata81(slock);
						break;
					case 82:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_122_lockdata82(slock);
						break;
					case 83:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_123_lockdata83(slock);
						break;
					case 84:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_124_lockdata84(slock);
						break;
					case 85:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_125_lockdata85(slock);
						break;
					case 86:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_126_lockdata86(slock);
						break;
					case 87:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_127_lockdata87(slock);
						break;
					case 88:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_128_lockdata88(slock);
						break;
					case 89:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_129_lockdata89(slock);
						break;
					case 90:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_130_lockdata90(slock);
						break;
					case 91:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_131_lockdata91(slock);
						break;
					case 92:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_132_lockdata92(slock);
						break;
					case 93:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_133_lockdata93(slock);
						break;
					case 94:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_134_lockdata94(slock);
						break;
					case 95:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_135_lockdata95(slock);
						break;
					case 96:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_136_lockdata96(slock);
						break;
					case 97:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_137_lockdata97(slock);
						break;
					case 98:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_138_lockdata98(slock);
						break;
					case 99:
						::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).set_139_lockdata99(slock);
						break;
					}
					}
				}
				
				m_wait++;
				if( m_wait > OPENANIME )
				{
					m_wait = 0;
					m_loop_exit = true;			//ポーズ終了
				}
				break;
			}

			
			local pos = getCurPosition(m_index);
			m_motion_cur.setPosition(pos.x, pos.y );
			m_motion_cur.exec();

			local pos = getSaveCurPosition(m_saveindex);
			m_motion_savecur.setPosition(pos.x, pos.y );
			m_motion_savecur.exec();


			//パワーオフ
			local fseq = ::g_frameCount.getFinishSeq();
	    if ( fseq > 0 ) 
	    {
//	      break; // モード終了
				m_seq = SEQ_CLOSE;
			}
			
			wait(0);
		}
		
		exit();
		
		local loop_result = { game_exit = m_child_result, cold_reset = m_cold_reset };

		::g_frameCount.logFileWrite("MenuPauseMain exec end");

		return( loop_result );
	}

	function getCurPosition( index )
	{
		local posx = [0,0,0,0];
		local posy = [-208,-148,168,228];
		
		
		local position = { x = posx[index], y = posy[index] };
		return( position );
	}
	function getSaveCurPosition( index )
	{
		local posx = [-390,-130,130,390];
		local posy = [62,62,62,62];
		
		
		local position = { x = posx[index], y = posy[index] };
		return( position );
	}
	
	function stateVisible( flg )
	{
		local i = 0;
		for ( i = 0; i < 4; i++ )
		{
			m_motion_StateDisp[i].setVisible(flg);
		}
	}


	function textVisible( flg )
	{
		m_caption.visible = flg;
		for( local i = 0; i < m_text.len(); i++ )
		{
			m_text[i].visible = flg;
		}
/*
		for( local i = 0; i < m_savetext.len(); i++ )
		{
			m_savetext[i].visible = flg;
		}
*/
		for( local i = 0; i < m_guide.len(); i++ )
		{
			m_guide[i].setVisible( flg );
		}
	}
	function createText( type, flg = true )
	{
		//開放する
		m_caption = null;
		m_guide = null;
		m_savetext = null;
		m_text = null;

    local font_path1 = getTitleMultiFontPath( );
    local font_path2 = getMultiFontPath();

		local offsetY1 = getFont32Yoffset();
		local offsetY2 = getFont24Yoffset();
		local textH = 60;

		local textDataGuide = null;
		local textData = null;
		local backY = 208;
		local backIndex = 8
		local guidenum = 4;
		switch( type )
		{
		case 0:
			textData = m_config["MAIN"];
			textDataGuide = m_config["GUIDE_MAIN"];
			break;
		case 1:
			textData = m_config["MAIN"];
			textDataGuide = m_config["GUIDE_SAVE"];
			break;
		}
		
		if( textData )
		{
			local str = get_item_string(textData["SettingMainText__CAPTION"]);

			m_caption = Indicator(m_layerText, s_rsc.find(font_path1));
			m_caption.visible = flg;
			m_caption.setRecognizeTag(true);
			m_caption.fontColor = TEXT_COLOR_NORMAL;
			m_caption.setAlignment(CONSOLE.ALIGNMENT_CENTER);
			m_caption.print(str);
//			m_caption.setCoord( 0, -276 - offsetY1);
			IndicatorSetCoord( m_caption, 0, -276 - offsetY1 );
		}
		if( textData )
		{
			local str = get_item_string(textData["SettingMainText__GUIDE"]);

			m_guide = Indicator(m_layerText, s_rsc.find(font_path2));
			m_guide.visible = flg;
			m_guide.setRecognizeTag(true);
			m_guide.fontColor = TEXT_COLOR_NORMAL;
			m_guide.setAlignment(CONSOLE.ALIGNMENT_CENTER);
			m_guide.print(str);
//			m_guide.setCoord( 0, 274 - offsetY1);
			IndicatorSetCoord( m_guide, 0, 274 - offsetY1 );
		}
		
		if( textData )
		{
			m_text = [];
			local i = 0;
			for ( i = 1; i < textData.len() - 1 ; i++ )
			{
				local idxname = "SettingMainText__" + i.tostring();
				printf( "idx = %s\n", idxname );
				local str = get_item_string(textData[idxname]);

				local idx = i - 1;
				m_text.append( Indicator(m_layerText, s_rsc.find(font_path2)) );
				m_text[idx].visible = flg;
				m_text[idx].setRecognizeTag(true);
				m_text[idx].fontColor = TEXT_COLOR_NORMAL;
				m_text[idx].setAlignment(CONSOLE.ALIGNMENT_CENTER);
				m_text[idx].print(str);
				
				if( idx >= 2 )
				{
//					m_text[idx].setCoord( 0, 168 - offsetY2 + ( idx - 2 ) * textH);
					IndicatorSetCoord( m_text[idx], 0, 168 - offsetY2 + ( idx - 2 ) * textH );
				}
				else
				{
//					m_text[idx].setCoord( 0, -208 - offsetY2 + idx * textH);
					IndicatorSetCoord( m_text[idx], 0, -208 - offsetY2 + idx * textH );
				}
				
				
				if( backIndex == idx )
				{
//					m_text[idx].setCoord( 0, backY - offsetY2 );
					IndicatorSetCoord( m_text[idx], 0, backY - offsetY2 );
				}
			}
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
				
				local posx = [-370,-80,180,380];
				local posw = [200,200,150,150];
				m_guide.append( MenuModeGuide(m_layerText, posx[i], 288, str1, str2, posw[i] ) );

			}
		}
	}

  // ステートロード直後に呼ばれる処理(override用)
  function after_StateLoad() 
  {
	}

	function after_StateSave()
	{
		//スクリーンショット更新
		_state_update();
		local i = 0;
		for ( i = 0; i < 4; i++ )
		{
			m_motion_StateDisp[i].setFileData( m_file_lists[i] );
			m_file_lists[i].thmubnail = null;
		}
	}

  function _check_input_quick_state_saveload(_mode) {
      return false;
  }
	

  function _create_view_thumbnail(_src_buf) {
    local thumbnail = null;
    if (_src_buf) {
			printf("_create_view_thumbnail\n" );
      thumbnail = RawTex(::util_get_menu_layerFolder(), STATE_DATA_THUMBNAIL_WIDTH, STATE_DATA_THUMBNAIL_HEIGHT, false);
      thumbnail.setVisible(false);
      thumbnail.smoothing = true;
      thumbnail.restore(_src_buf);
    }
    return thumbnail;
  }

	//ステート情報取得
  function _state_update() 
  {
    local _res = 0;
    {

      local backupStatedata = BackupStatedata(null, false); // エラー表示なし

			m_file_lists = null;
			m_file_lists = [];


      //4つごとにデータが格納される。
			printf("m_saveindex=%d\n", m_saveindex );
			printf("m_current_lists_index=%d\n", m_current_lists_index );

			local i = 0;
			for ( i = 0; i < 4; i++ )
			{
	      local _info           = null;
	      local file_struct     = null;
	      file_struct = backupStatedata.get_state_data_info(i + ( m_current_lists_index * 4 ));
	      _info = _get_state_info(file_struct);
	      file_struct = null;
	      m_file_lists.append({ name = format("%s", _info.exist ? _info.date : "------------"), 
	                          exist = _info.exist, 
	                          date = _info.date, 
	                          thmubnail = this._create_view_thumbnail(_info.thmubnail) });
	    }
			printf("m_file_lists[0].name = %s\n", m_file_lists[0].name );
			printf("m_file_lists[1].name = %s\n", m_file_lists[1].name );
			printf("m_file_lists[2].name = %s\n", m_file_lists[2].name );
			printf("m_file_lists[3].name = %s\n", m_file_lists[3].name );
			

    }
  }

  function _get_state_info(_struct) {
    local _exist, _date = null, _time = null, _thmubnail = null;
    local tick_HI, tick_LW;
    local boot_time;
    local _h, _m, _s;

    _exist = _struct ? true : false;
    if (_exist) {
      tick_HI    = _struct.root._01_added_value._01_tick_HI;
      tick_LW    = _struct.root._01_added_value._02_tick_LW;
      boot_time  = _struct.root._01_added_value._08_boot_timer;
      _h         = (boot_time / (60 * 60 * 60)) % 60;
      _m         = (boot_time / (60 * 60)) % 60;
      _s         = ((boot_time % 60) / 60.0 * 100.0).tointeger();
//      _date      = SystemEtc.getStringLocalTime(tick_HI, tick_LW);
      //言語によって日付の並びをかえる
	    local clock = SystemEtc.getClockLocalTime(tick_HI, tick_LW);
/*
			local langTag = ::sys_get_language_tag();
			switch (langTag) 
			{
			case LANGUAGE_TAG_FRENCH:
		    _date = format("%02d/%02d/%04d %02d:%02d:%02d"
		                      , clock["day"]
		                      , clock["month"]
		                      , clock["year"]
		                      , clock["hour"]
		                      , clock["minute"]
		                      , clock["second"]);
				break;
			case LANGUAGE_TAG_GERMAN:
		    _date = format("%02d.%02d.%04d %02d:%02d:%02d"
		                      , clock["day"]
		                      , clock["month"]
		                      , clock["year"]
		                      , clock["hour"]
		                      , clock["minute"]
		                      , clock["second"]);
				break;
			case LANGUAGE_TAG_SPANISH:
		    _date = format("%02d-%02d-%04d %02d:%02d:%02d"
		                      , clock["day"]
		                      , clock["month"]
		                      , clock["year"]
		                      , clock["hour"]
		                      , clock["minute"]
		                      , clock["second"]);
				break;

			default:
				//ディフォルトは英語
		    _date = format("%02d/%02d/%04d %02d:%02d:%02d"
		                      , clock["month"]
		                      , clock["day"]
		                      , clock["year"]
		                      , clock["hour"]
		                      , clock["minute"]
		                      , clock["second"]);
				break;
			}
*/
				//ディフォルトは英語
		    _date = format("%04d/%02d/%02d"
		                      , clock["year"]
		                      , clock["month"]
		                      , clock["day"]
											);
      _thmubnail = ::BackupStatedata.st_get_thumbnail_from_struct(_struct);
    } else {
      _date      = "";
      _time      = "";
      _thmubnail = null;
    }

    return { exist = _exist, date = _date, time = _time, thmubnail = _thmubnail };
  }

	//ポーズ解除
	function _resume()
	{
		m_seq = SEQ_CLOSE;
	}

	//ゲーム終了
  function _return_to() {
    // ゲーム終了確認
    local nosey = 0 != ::g_systemdata.get_data_setting_etc__nosey_normal_mode();
		if( TITLESELECTTEST != 0 )
		{
			//タイトル連続再生テストの場合は確認をださない
			m_child_result = true;
		}
		else
		{
	    m_child_result = ::confirm_terminate__play_standalone(m_session, m_play_replay, true, m_disable_state_save, nosey);
	  }
		if( m_child_result == true )
		{
			::g_emu_task.visible = false;
//			::g_wallpaperBG.open();	//背景を残しておく
//			::delete_wallpaper();
			m_seq = SEQ_CLOSE;
		}
			
  }
	
  // 通常のゲームリセット
  function _reset_game() {
		if ( ::confirm_dialog("NoticeMsg__GAME_RESET") ) 
		{
	    local current_replay_mode = ::g_emu_task.replayMode;
			m_seq = SEQ_CLOSE;

	    ::request_reset_emulator();
	    ::play_standalone_try_reset_emulator(m_play_replay, m_title_control, "hot_reset"); 

			m_cold_reset = false;
		}
  }

	function exit()
	{
		m_motion_StateDisp = null;
		::release_savedata_capture();

	}

};

class MenuPauseMain extends MenuPauseBase {
};



//タイトルパッケージクラス
class PauseCorsol 
{
	m_motion = null;
	m_layer = null;

	m_posx = null;
	m_posy = null;

	m_targetposx = null;
	m_targetposy = null;

	m_dir = null;
	m_spd = null;

	m_alpha = null;
	m_seq = null;

	m_type = null;
	m_isVisible = null;

  constructor( ) 
  {
		::g_frameCount.logFileWrite("PauseCorsol constructor start");

		m_type = 0;
		m_isVisible = true;

    m_layer = ScaledLayer(::util_get_menu_layerFolder());
    m_layer.visible = true;
    m_layer.smoothing = MOTSMOOTHING;
		m_layer.priority = PRI_CSL;

    local motion_path = conv_path(s_pause_motionPath);

    m_layer.registerMotionResource(s_rsc.find(motion_path));  // レイヤにモーションリソースを登録

		m_alpha = 255;
		
		m_posx = 0;
		m_posy = 0;

		m_targetposx = 0;
		m_targetposy = 0;

		m_dir = 0;
		m_spd = 0;

		m_seq = 0;

  	_init( );

		::g_frameCount.logFileWrite("PauseCorsol constructor end");
  }

  function exec() 
  {

		switch( m_seq )
		{
		case 0:
			//移動
			//自機に向かってホーミング
			m_dir = ::PointDir( m_posx, m_posy, m_targetposx, m_targetposy );
			m_spd = ::TragetKinspeed_Option( m_posx, m_posy, m_targetposx, m_targetposy );
			local pos = ::DirMove( m_dir, m_spd, m_posx, m_posy );
			m_posx = pos.x;
			m_posy = pos.y;

			if( m_spd == 0 )
			{
				m_posx = m_targetposx;
				m_posy = m_targetposy;
				m_seq++;
			}

			break;
		default:
			//移動終了
			m_posx = m_targetposx;
			m_posy = m_targetposy;
			m_spd = 0;
			break;
		}

		//表示座標更新
		m_motion.left = m_posx;
		m_motion.top = m_posy;
		
  }
  
  function _init() 
  {
		::g_frameCount.logFileWrite("PauseCorsol _init start");

		m_motion         = Motion(m_layer);
		m_motion.chara   = "cursol";
		
		m_motion.motion  = "pause";

		m_motion.opacity = 255;
		m_motion.visible = true;
		m_motion.independentLayerInherit = true;
		m_motion.progress();
		m_motion.left = 0;	//0,0が画面中央
		m_motion.top = 0;

		::g_frameCount.logFileWrite("PauseCorsol _init end");
  }
  
  function setPosition( x, y ) 
  {
		m_targetposx = x;
		m_targetposy = y;
		m_seq = 0;
  }
  //------------------------------
  // レイヤのプライオリティを設定
  //------------------------------
  function setLayerPriority(_priority) {
    if (m_layer != null) m_layer.priority = _priority;
  }

  function setVisible( flg ) 
  {
		m_motion.visible = flg;
  }


  //設定ボタン用のモーションにするか？
  function setSaveCorsol( )
  {
		m_motion.motion  = "save";
	}
  //設定ボタン用のモーションにするか？
  function setScreenCorsol( flg )
  {
		switch( flg )
		{
		case 0:
			m_motion.motion  = "pause";
			break;
		case 1:
			m_motion.motion  = "screen";
			break;
		case 2:
			m_motion.motion  = "screen_back";
			break;
		}
	}
};

//ステート表示クラス
class StateDisp 
{
	m_motion_lock = null;
	m_motion_lamp = null;
	m_layer = null;

	m_type = null;
	m_isVisible = null;

	m_view = null;

  m_posx = null;
  m_posy = null;
  
	m_text_date = null;
	m_saveAnime = null;

  constructor( ) 
  {
		m_posx = 0;
		m_posy = 0;
		m_type = 0;
		m_isVisible = true;
		m_saveAnime = 0;

    m_layer = ScaledLayer(::util_get_menu_layerFolder());
    m_layer.visible = true;
    m_layer.smoothing = MOTSMOOTHING;
		m_layer.priority = PRI_STTEXT;

    local motion_path = conv_path(s_pause_motionPath);
    local font_path = conv_path(MENU_MULTI_FONT_PATH);

    m_layer.registerMotionResource(s_rsc.find(motion_path));  // レイヤにモーションリソースを登録

		m_text_date = Indicator(m_layer, s_rsc.find(font_path));
		local _str = "";
		m_text_date.visible = false;
		m_text_date.setRecognizeTag(true);
		m_text_date.fontColor = TEXT_COLOR_NORMAL;
		m_text_date.setAlignment(CONSOLE.ALIGNMENT_CENTER);
		m_text_date.print(_str);

  	_init( );
  }
  
  function _init() 
  {
		m_motion_lock         = Motion(m_layer);
		m_motion_lock.chara   = "pause";
		m_motion_lock.motion  = "lock";

		m_motion_lock.opacity = 255;
		m_motion_lock.visible = true;
		m_motion_lock.independentLayerInherit = true;
		m_motion_lock.progress();
		m_motion_lock.left = 0;	//0,0が画面中央
		m_motion_lock.top = 0;

		m_motion_lamp         = Motion(m_layer);
		m_motion_lamp.chara   = "pause";
		m_motion_lamp.motion  = "lamp";

		m_motion_lamp.opacity = 255;
		m_motion_lamp.visible = true;
		m_motion_lamp.independentLayerInherit = true;
		m_motion_lamp.progress();
		m_motion_lamp.left = 0;	//0,0が画面中央
		m_motion_lamp.top = 0;
  }
  
  function setPosition( x, y ) 
  {
		m_posx = x;
		m_posy = y;

		m_motion_lock.left = x;
		m_motion_lock.top = y;
		
		m_motion_lamp.left = x;
		m_motion_lamp.top = y;

//		m_text_date.setCoord( x, y + DATETXTOFS_Y );
		IndicatorSetCoord( m_text_date, x, y + DATETXTOFS_Y );

  }
  function setFileData( file, init = false ) 
  {
    m_view = file.thmubnail;
    if (m_view) {
			printf("m_view = %d\n", m_layer.priority + 1 );
			
			local _scale = 0.5;
      m_view.setVisible(true);
      m_view.setColor(1.0, 1.0, 1.0);
      m_view.setScale(_scale, _scale);
      m_view.setOffset(-m_posx + STIMAGEOFS_X, m_posy + STIMAGEOFS_Y);
      m_view.setOpacity(255);
      m_view.priority = PRI_STIMAGE;

			if( init == true )
			{
				m_saveAnime = 1;
				m_motion_lamp.setVariable("savelamp", 1);
			}
			else
			{
				if( m_saveAnime == 0 )
				{
					m_motion_lamp.setVariable("savelamp", 2);
				}
				else
				{
					m_motion_lamp.setVariable("savelamp", 1);
				}
				m_saveAnime = 1;
			}
    }
    else
    {
			m_saveAnime = 0;
			m_motion_lamp.setVariable("savelamp", 0);
		}

		local _str = file.name;
		m_text_date.print(_str);

  }
  //------------------------------
  // レイヤのプライオリティを設定
  //------------------------------
  function setLayerPriority(_priority) {
    if (m_layer != null) m_layer.priority = _priority;
  }

  function setLock( flg ) 
  {
		m_motion_lock.visible = flg;
		if( flg == true )
		{
				m_motion_lock.setVariable("savelock", 2);
		}
		else
		{
				m_motion_lock.setVariable("savelock", 0);
		}

		
  }
  function setVisible( flg ) 
  {
		m_layer.visible = flg;
    if (m_view) 
    {
			m_view.setVisible(flg);
		}
  }

};


