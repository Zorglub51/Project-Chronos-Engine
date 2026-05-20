//----------------------------------------------------------------------
// 定数テーブル
//----------------------------------------------------------------------
const MENU_SE_BANK_PATH_BASE           = "system/sound/se_menu";
const MENU_BGM_PATH_BASE               = "system/sound/bgm_menu";
const MENU_OPENING_BGM_PATH_BASE       = "system/sound/bgm_logo";

//const MENU_MULTI_FONT_PATH             = "system/font/multi32_0.psb";
const MENU_MULTI_FONT_PATH             = "system/font/makoto_basefont.psb";	//フォントの変更
const MENU_MULTI_FONT18_PATH           = "system/font/makoto_basefont_18pt.psb";	//フォントの変更
const MENU_MULTI_FONT32_PATH           = "system/font/makoto_basefont_32pt.psb";	//フォントの変更

const MENU_US_MULTI_FONT_PATH             = "system/font/makoto_basefont_eng.psb";	//フォントの変更
const MENU_US_MULTI_FONT18_PATH           = "system/font/makoto_basefont_18pt_eng.psb";	//フォントの変更
const MENU_US_MULTI_FONT32_PATH           = "system/font/makoto_basefont_32pt_eng.psb";	//フォントの変更

//const MENU_MULTI48_FONT_PATH           = "system/font/multi48.psb";
const MENU_MULTI48_FONT_PATH           = "system/font/makoto_basefont_32pt.psb";
const MENU_FIXED_FONT_PATH             = "system/font/fixed32.psb";
const MENU_TRIAL_FONT_PATH             = "system/font/fixed64_trial.psb";
const MENU_BACKUPUI_FONT_PATH          = "system/font/multi32_1.psb"; // BackupList用
const MENU_BG_MOTION_PATH              = "system/motion/bg00.psb";
const MENU_WIPE_MOTION_PATH            = "system/motion/wipe.psb";
const MENU_NOWLOADING_MOTION_PATH      = "system/motion/nowloading.psb";
const MENU_STAFF_MOTION_PATH           = "system/motion/staff_credit.psb";
const MENU_DIALOG_MOTION_PATH          = "system/motion/menu_dialog.psb";
const MENU_ITEM_INDICATOR_MOTION_PATH  = "system/motion/itemindicator.psb";

const CONFIG_SYSTEM_PROFILE_PATH       = "system/config/system_prof.psb";
const CONFIG_GAME_CONTENT_PATH         = "system/config/game_content.psb";
const CONFIG_GENRE_CODE_PATH           = "system/config/genre_code.psb";
const CONFIG_FONT_ICON_CODE_PATH       = "system/config/font_icon_code.psb";
const CONFIG_NOTICE_PATH               = "system/config/notice.psb";
const CONFIG_MENU_ITEM_PATH            = "system/config/menu_item.psb";
const CONFIG_MENU_INFO_PATH            = "system/config/menu_info.psb";
const CONFIG_MENU_STR_REPLACE_PATH     = "system/config/menu_str_replace.psb";
const CONFIG_MENU_CURSOR_PATH          = "system/config/menu_cursor.psb";
const CONFIG_GUIDE_PARTS_PATH          = "system/config/guide_parts.psb";
const CONFIG_GUIDE_BODY_PATH           = "system/config/guide_body.psb";
const CONFIG_KINSOKU_PATH              = "system/config/kinsoku.psb";
const CONFIG_SESSION_PATH              = "system/config/session.psb";
const CONFIG_SETTING_SCREEN_PATH       = "system/config/setting_screen.psb";
const CONFIG_MODE_MAIN_PATH            = "system/config/mode_main.psb";

const STRUCT_SAVEDATA_DUMMY_PATH       = "system/config/struct_savedata_dummy.psb";
const STRUCT_SYSTEMDATA_PATH           = "system/config/struct_systemdata.psb";
const STRUCT_SYSTEMDATA_TITLE_PATH     = "system/config/struct_systemdata_title.psb";
const STRUCT_SYSTEMDATA_TITLES_PATH    = "system/config/struct_systemdata_titles.psb";
const STRUCT_SOUND_PLAY_LIST_PATH      = "system/config/struct_sound_play_list.psb";
const STRUCT_STATEDATA_PATH            = "system/config/struct_statedata.psb";
const STRUCT_STATEDATA_L_PATH            = "system/config/struct_statedata_l.psb";
const STRUCT_REPLAYDATA_PATH           = "system/config/struct_replaydata.psb";
const STRUCT_SRAM_IMAGE_PATH           = "system/config/struct_sram_image.psb";
const STRUCT_STATEDATA_EXTEND_PATH     = "system/config/struct_statedata_extend.psb";
const STRUCT_SETTING_GAME_IMAGE_PATH   = "system/config/struct_setting_game_image.psb";

const STRUCT_RULE_SETTING_PATH         = "system/config/struct_rule_setting.psb";
const STRUCT_RULE_PLAYSIDE_PATH        = "system/config/struct_rule_playside.psb";

const SCRIPT_PLAY_STANDALONE_PATH      = "system/script/play_standalone.nut";
const SCRIPT_PLAY_RANKINGATTACK_PATH   = "system/script/play_rankingattack.nut";
const SCRIPT_PLAY_NETWORK_PATH         = "system/script/play_match.nut";
const SCRIPT_PLAY_REPLAY_PATH          = "system/script/replay_play.nut";
const SCRIPT_MODE_MAIN_PATH            = "system/script/mode_main.nut";
const SCRIPT_MODE_TITLE_SELECT_PATH    = "system/script/mode_title_select.nut";
const SCRIPT_MODE_NORMAL_PATH          = "system/script/mode_normal.nut";
const SCRIPT_MODE_NETWORK_PATH         = "system/script/mode_network.nut";
const SCRIPT_MODE_RANKING_PATH         = "system/script/mode_ranking.nut";
const SCRIPT_MODE_REPLAY_PATH          = "system/script/mode_replay.nut";
const SCRIPT_MODE_MISC_PATH            = "system/script/mode_misc.nut";
const SCRIPT_MODE_OPTION_PATH          = "system/script/mode_option.nut";
const SCRIPT_MODE_LEADERBOARD_PATH     = "system/script/mode_leaderboard.nut";
const SCRIPT_MODE_STAFF_PATH           = "system/script/mode_staff.nut";
const SCRIPT_SOFTWARE_MANUAL_PATH      = "system/script/mode_manual.nut";
const SCRIPT_SETTING_CONTROLLER_PATH   = "system/script/mode_setting_pad.nut";
const SCRIPT_PAUSE_MAIN_PATH           = "system/script/pause_main.nut";
const SCRIPT_PAUSE_OPTION_PATH         = "system/script/pause_option.nut";
const SCRIPT_PAUSE_HELPANDOPTIONS_PATH = "system/script/pause_help_and_options.nut";
const SCRIPT_SETTING_SCREEN_PATH       = "system/script/setting_screen.nut";
const SCRIPT_SETTING_SCREEN_SUB_PATH   = "system/script/setting_screen_sub.nut";
const SCRIPT_SETTING_SCREEN_SYS_PATH   = "system/script/setting_screen_sys.nut";
const SCRIPT_SETTING_SOUND_PATH        = "system/script/setting_sound.nut";
const SCRIPT_SETTING_SAVEDATA_PATH     = "system/script/setting_savedata.nut";
const SCRIPT_SETTING_ETC_PATH          = "system/script/setting_etc.nut";
const SCRIPT_SETTING_OTHERS_PATH       = "system/script/setting_others.nut";
const SCRIPT_SETTING_GAME_PATH         = "system/script/setting_game.nut";
const SCRIPT_SESSION_UI_SETTING_PATH   = "system/script/session_ui_setting.nut";

// title
const MENU_OPENING_MOTION_PATH         = "motion/opening.psb";
const MENU_COMMAND_FACE_MOTION_PATH    = "motion/face.psb";
const MENU_EMU_SCREEN_MOTION_PATH      = "motion/emu_screen.psb";
const MENU_EMU_SCREEN_STATE_PATH       = "state/emu_screen.bin";
const CONFIG_TITLE_PROFILE_PATH        = "config/title_prof.psb";
const CONFIG_TITLE_PROFILE_SPEC_PATH   = "config/title_prof_specdepend.psb";
const CONFIG_TITLE_MENU_STRING_PATH    = "config/title_menu_string.psb";
const CONFIG_COMMAND_LIST_PATH         = "config/command_list.psb";
const CONFIG_TITLE_OPENING_PATH        = "config/mode_opening.psb";
const CONFIG_TITLE_LOGO_PATH           = "config/mode_logo.psb";
const CONFIG_STATE_CHECK_PATH          = "config/title_state_check.psb";
const CONFIG_TITLE_MANUAL_PATH         = "config/title_manual_%s.psb";
const CONFIG_TITLE_SETTING_GAME_PATH   = "config/title_setting_game.psb";
const CONFIG_TITLE_SAVE_STRING_PATH    = "config/title_savedata_string.psb";
const CONFIG_TITLE_LEADERBOARDS_PATH   = "config/title_leaderboards.psb";
const CONFIG_TITLE_ATTACHMENTDATA_PATH = "config/title_attachmentdata.psb";
const CONFIG_TITLE_MEDAL_SYSTEM_PATH   = "config/title_medal_system.psb";
const CONFIG_TITLE_SESSION_PATH        = "config/title_session.psb";
const SCRIPT_TITLE_MODE_TOP_PATH       = "script/title_mode_top.nut";
const SCRIPT_TITLE_MODE_SELECT_PATH    = "script/title_mode_title_select.nut";
const SCRIPT_TITLE_PAUSE_MAIN_PATH     = "script/title_pause_main.nut";

const MENU_UI_TITLESELECT_WARNING_MOTION_PATH     = "system/motion/titleselect_warning.psb";
const MENU_UI_TITLESELECT_LOGO_MOTION_PATH     = "system/motion/titleselect_logo.psb";
const MENU_UI_TITLESELECT_TITLE_MOTION_PATH     = "system/motion/titleselect_title.psb";
const MENU_UI_TITLESELECT_MOTION_PATH     = "system/motion/titleselect_ui.psb";
const MENU_UI_TITLESELECT_PAUSE_MOTION_PATH     = "system/motion/titleselect_pause.psb";
const MENU_JP_TITLESELECT_JP_MOTION_PATH  = "motion/title_jp_titleselect_jp.psb";
const MENU_JP_TITLESELECT_US_MOTION_PATH  = "motion/title_jp_titleselect_us.psb";
const MENU_US_TITLESELECT_JP_MOTION_PATH  = "motion/title_us_titleselect_jp.psb";
const MENU_US_TITLESELECT_US_MOTION_PATH  = "motion/title_us_titleselect_us.psb";

const MENU_UI_WALLPAPER01_MOTION_PATH     = "system/motion/wall_bg01.psb";
const MENU_UI_WALLPAPER02_MOTION_PATH     = "system/motion/wall_bg02.psb";
const MENU_UI_WALLPAPER03_MOTION_PATH     = "system/motion/wall_bg03.psb";
const MENU_UI_WALLPAPER04_MOTION_PATH     = "system/motion/wall_bg04.psb";
const MENU_UI_WALLPAPER05_MOTION_PATH     = "system/motion/wall_bg05.psb";
const MENU_UI_WALLPAPER06_MOTION_PATH     = "system/motion/wall_bg06.psb";

const SCREEN_XSIZE = 1280;
const SCREEN_YSIZE = 720;

const KEYWAIT = 10;	//キー受け付け間隔
const MODE_DEMO_DEBUGDISP = 0;	//デモ調整用デバッグ表示を行う場合は1
const SCALETEST = 0;	//スケール変更テストを行う場合は1
const TITLESELECTTEST = 0;	//連続タイトル選択をテストを行う場合は1
const LINEUPCHANGETEST = 0;	//ラインナップ切替連続テストを行う場合は1

// テキストカラー関係
const TEXT_COLOR_NORMAL  = 0xffffffff;
const TEXT_COLOR_GRAYOUT = 0x808080ff

// 音量関連
const BGM_LOGO_VOLUME  = 1.0; // 0.0 ~ 1.0
const BGM_TITLE_VOLUME = 1.0; // 0.0 ~ 1.0
const BGM_MAIN_VOLUME  = 1.0; // 0.0 ~ 1.0


// フレームタイミング関連
const FRAME_KEY_REPEAT_FIRST     = 18;
const FRAME_KEY_REPEAT_NEXT      =  3;
const FRAME_MENU_BG_FADE         = 16; // 壁紙の切り替わり速度
const FRAME_MANUAL_FADE          = 16; // マニュアルのページ切り替わり速度
const FRAME_EMU_TASK_FADE        =  8; // エミュレータ画面の切り替わり速度
const FRAME_SAVELOAD_WIPE        =  8; // セーブロード時の全画面ワイプ速度
const FRAME_DIALOG_ZOOM          =  8; // ダイアログの標準拡大縮小速度
const FRAME_MENU_SETTING_ZOOM    =  6; // 設定メニューの拡大縮小速度
const FRAME_SETTING_PAD_FADE     =  6; // コントローラ設定時の切り替わり速度
const FRAME_SUBITEM_WINDOW_ZOOM  =  6; // 設定個別メニューの切り替わり速度
const FRAME_MENU_INFO_ZOOM       =  6; // 情報表示フレームの切り替わり速度
const FRAME_MENU_SELECTLIST_ZOOM =  6; // リスト選択フレームの切り替わり速度
const FRAME_LEADERBOARD_ZOOM     =  6; // リーダーボード表示の切り替わり速度

const FRAME_BGM_TITLE_FADE_OUT   = 15;
const FRAME_BGM_MAIN_FADE_OUT    = 15;


// リプレイ記録最大分(STRUCT_REPLAYDATA_PATHのdata_sizeに依存)
const MINUTES_REPLAY_RECORD  = 360;


// プラットフォームの配信仕向け地定義
const PACKAGE_REGION_JAPAN  = "japan";
const PACKAGE_REGION_USA    = "usa";
const PACKAGE_REGION_EUROPE = "europe";
const PACKAGE_REGION_ASIA   = "asia";


// エミュレータのゲームバージョン定義(これ以外にタイトル個別定義も認める)
const GAME_REGION_JAPAN  = "JAPAN";
const GAME_REGION_USA    = "USA";
const GAME_REGION_EUROPE = "EUROPE";


// 言語設定
const LANGUAGE_TAG_JAPANESE   = "jpn";
const LANGUAGE_TAG_ENGLISH    = "eng";
const LANGUAGE_TAG_ENGLISH_UK = "eng-uk"; // イギリス英語
const LANGUAGE_TAG_FRENCH     = "fra";
const LANGUAGE_TAG_ITALIAN    = "ita";
const LANGUAGE_TAG_GERMAN     = "ger";
const LANGUAGE_TAG_SPANISH    = "spa";
const LANGUAGE_TAG_DUTCH      = "dut";
const LANGUAGE_TAG_PORTUGUESE = "por";
const LANGUAGE_TAG_RUSSIAN    = "rus";
const LANGUAGE_TAG_CHINA      = "chn";
const LANGUAGE_TAG_KOREA      = "kor";

//フレームの種類
const FRAME_PCENGINE    = 0;
const FRAME_COREGRAFX   = 1;

// ランキング期間定義
const LEADERBOARD_TIME_FILTER_ALL     = "all";
const LEADERBOARD_TIME_FILTER_MONTHLY = "monthly";
const LEADERBOARD_TIME_FILTER_WEEKLY  = "weekly";

// ランキングプレイヤー種別定義
const LEADERBOARD_PLAYER_FILTER_ALL    = "all";
const LEADERBOARD_PLAYER_FILTER_FRIEND = "friend";
const LEADERBOARD_PLAYER_FILTER_MINE   = "mine";

// ランキングシステム予約総合種別名
const LEADERBOARD_KIND_OVERALL         = "SYSTEM_OVERALL";

// ランキングの追加情報カラム名
const LEADERBOARD_COLUMN_ID_SUB_KIND   = "SubKind";


const PRIORITY_LAYER_FOLDER_MENU_NOTIFY =  1000;
const PRIORITY_LAYER_FOLDER_MENU_BACKUP =   900;
const PRIORITY_LAYER_FOLDER_MENU_NORMAL =     0;
const PRIORITY_LAYER_FOLDER_GAME_SCREEN =  -100;
const PRIORITY_LAYER_FOLDER_WALLPAPER   = -9999;

const PRIORITY_DEBUG_LAYER         = 10000;
const PRIORITY_WIPE_LAYER          =  9995;
const PRIORITY_NOTIFY_DIALOG_LAYER =  9901;
const PRIORITY_NOTIFY_LAYER        =  9900;
const PRIORITY_BOOTLOGO_LAYER      =  9800;
const PRIORITY_NOWLOADING_LAYER    =  9000;
const PRIORITY_ITEMINDICATOR_LAYER =  1000;
const PRIORITY_SUBMENU_LAYER       =    10;
const PRIORITY_EMULATOR_LAYER      = -9990;
const PRIORITY_BG_LAYER            = -9999;


const MOTION_COLOR_WEIGHT_CENTER_VALUE_ALPHA = 0xff; // α成分中間値
const MOTION_COLOR_WEIGHT_CENTER_VALUE_COLOR = 0x80; // 色成分中間値
const MOTION_COLOR_WEIGHT_BASE_VALUE_COLOR   = 0xff; // 色成分1.0値
const MOTION_COLOR_WEIGHT_WHITE              = 0x808080ff;
const MOTION_COLOR_WEIGHT_BLACK              = 0x000000ff;


const WIPE_OPACITY_MOVIE_PREVIEW = 0x00c0;


// 操作関連
const EMU_BUTTON_NUM_MAX = 12;        // エミュレータキーコンフィグでサポートするボタン種類最大数

const TOUCH_CHECKER_RESULT_TOUCHED  = "::touched";
const TOUCH_CHECKER_RESULT_TOUCHING = "::touching";
const TOUCH_CHECKER_RESULT_RELEASED = "::released";
const TOUCH_CHECKER_RESULT_DOUBLED  = "::doubled";
const TOUCH_CHECKER_RESULT_SWIPED   = "::swiped";
const TOUCH_CHECKER_RESULT_DRAGGING = "::dragging";

//言語
const LANGUAGE_JPN = 0;
const LANGUAGE_ENG = 1;
const LANGUAGE_SPA = 2;
const LANGUAGE_FRA = 3;
const LANGUAGE_ITA = 4;
const LANGUAGE_GER = 5;
const LANGUAGE_CHA = 6;
const LANGUAGE_KOR = 7;

//本体リージョン
const DEVID_JP = 40;
const DEVID_US = 41;


//スムージング
const MOTSMOOTHING = 0;

//リージョン
const PKGREGION_JP = 0;
const PKGREGION_US = 1;
const PKGREGION_EU = 2;

//ラインナップ
const LINEUP_JP = 0;	//日本版
const LINEUP_US = 1;	//海外版
const LINEUPMAX = 50;	//1ラインナップあたりのスロット数 (0..49 jp, 50..99 us in title_mode_top)

const DEMOVERSISON = 0;	//デモバージョンの場合は1
const BGCOUNT_WARI = 4.0;	//BGの移動速度

//操作なしで画面を暗くするまでの時間(フレーム)
const NOINPUT_BRIGHTNESS_COUNT = 10800;

const SCREENSETTING_GT = 4;	//スクリーン設定GT
const SCREENSETTING_MAX = 5;	//スクリーン設定数

const BGBACK_NUM = 1;
//----------------------------------------------------------------------
// backup関連
//----------------------------------------------------------------------
// セーブデータType
enum BackupSegmentTypes {
  // system
  SYSTEM_DATA,
  SYSTEM_ARCHIVE,
  SOUND_PLAY_LIST,
  _DUMMY_03,
  _DUMMY_04,
  _DUMMY_05,
  _DUMMY_06,
  _DUMMY_07,
  // title
  TITLE_DATA,
  TITLE_SOUND_PLAY_LIST,
  _DUMMY_10,
//  _DUMMY_11,
  EMU_STATE_L,
  EMU_STATE,
  EMU_REPLAY,
  // 複数まとめパックでは以降(タイトル数-1)*NUM_EMU_DATA_SEGMENT_TYPES分使用する
};

const NUM_EMU_DATA_SEGMENT_TYPES = 2; // 複数まとめパックでタイトル毎に個別に使用するセグメント種類数


// セーブデータType毎のデータ個数
enum BackupSegmentNum {
  // system
  SYSTEM_DATA           = 1,
  SYSTEM_ARCHIVE        = 1,
  SOUND_PLAY_LIST       = 2, // SVCPS3では現在リリース時期で2versionあり

  // title
  TITLE_DATA            = 1,
  TITLE_SOUND_PLAY_LIST = 1,
  EMU_STATE_L           = 500,
  EMU_STATE             = 500,
  EMU_REPLAY            = 99,
};

// セーブデータ構造体のフォーマットバージョン
enum BackupStructVersion {
  // system
  SYSTEM_DATA           = 0x00010012,
  SYSTEM_ARCHIVE        = 0x00010010,
  SOUND_PLAY_LIST       = 0x00010010,

  // title
  TITLE_DATA            = 0x00010018,
  TITLE_SOUND_PLAY_LIST = 0x00010010,
  EMU_STATE_L           = 0x00010010,
  EMU_STATE             = 0x00010010,
  EMU_REPLAY            = 0x00010010,
};

enum BackupGameMode {
  NORMAL  = 0,  
  RANKING = 1,  
  NETWORK = 2,  
};

enum BackupReplaydataBitFlag {
  EMULATE_ORIGINAL_BUG = 0x0001,
  DISABLE_SNS          = 0x0002,
};

const SHARED_DATA_FLAGTABLE_NAME = "#shared_flag#";

//----------------------------------------------------------------------
// システムセーブデータ関連
//----------------------------------------------------------------------
// 管理情報index
enum SystemDataValueIndex {
  SETTING_PAD,
  SETTING_SCREEN,
  SETTING_SOUND,
  SETTING_SAVEDATA,
  SETTING_ETC,
  SETTING_NETWORK,
  SETTING_GAME,
  BACKUP_FLAGS,
  WINDOW_INFO,
  PURCHASE_RECORD,
  WORK_MEDAL,
  WORK_TRIAL,
  SRAM_DATA,
  
  ENUM_MAX_NUM,
};

// 処理filter
enum SystemDataOpeBitFlag {
  COMMON = 0x01,
  TITLE  = 0x02,
  BOTH   = 0x03,
};

// 最後に遊んだゲームモード
enum LastPlayMode {
  NORMAL_MODE,	// default
  QUICK_MATCH,
  COOP_MATCH,
  PLAYER_MATCH,
  CUSTOM_SEARCH,
  CREATE_SESSION,
  RANKING_ATTACK,

  // (システムセーブデータ互換性の維持のため、必ずケツに追加すること！！)

  ENUM_MAX_NUM
};

// 汎用個別タイトル用BitFlag
enum SystemDataTitleBitFlag {
  NOTIFY_STEREO_3D = 0x00000001,
};

//----------------------------------------------------------------------
// ネットワークモード関連
//----------------------------------------------------------------------
// ネットワークモードのメニュータイプ
enum NetworkModeMenuTypes {
  QUICK_MATCH,      // クイックマッチ
  COOP_MATCH,       // 協力プレイ
  PLAYER_MATCH,     // 対戦プレイ
  CUSTOM_SEARCH,    // カスタム(検索)
  CREATE_SESSION,   // クリエイトセッション
  INVITE_MATCH,     // 招待(された)マッチ

  ENUM_MAX_NUM
};

// セッション処理type
//  変更時は sesion_common.nut SessionOperateTypesTag getResultMessage の対応メッセージも変えること
enum SessionOperateTypes {
  QUICK_MATCH,          // クイックマッチ
  CUSTOM_SEARCH,        // カスタム(検索)
  JOIN_SESSION,         // (検索)セッションに接続
  CREATE_SESSION,       // クリエイトセッション
  CREATE_IV_SESSION,    // クリエイトセッション（招待）
  WAIT_IV_RESULT,       // 招待リザルト待ち
  INVITE_MATCH,         // 招待(された)マッチ
  KEEP_SESSIONT,        // セッション維持
  CANCEL,               // キャンセル処理中
  ERROR,                // エラー処理中
  TERMINATED,           // 正常終了状態
  GENERAL_DUMMY,        // (type外でエラーダイアログを出す用)

  ENUM_MAX_NUM
};

// 共通なセッション設定項目の最大数
const COMMON_SESSION_SETTING_NUM_MAX = 4;
// タイトル依存なセッション設定項目の最大数
const TITLE_SESSION_SETTING_NUM_MAX = 6;
// 検索セッション情報リストの最大数
const SESSION_INFO_LIST_NUM_MAX = 12; // （リスト表示最大数も兼ねる）


// セッション処理完了待ちチェック（リザルトダイアログ付き）の戻り値
enum SessionWaitCheckResult {
  OPERATING_OR_NONE,     // 処理中or未処理

  COMPLETED,             // 正常終了
  ERROR_DISCONNECT,      // エラー（切断）
  ERROR_CANCEL,          // キャンセル
  ERROR_TIMEOUT,         // タイムアウト
  ERROR_LOGOUT,          // 意図的な切断
  ERROR_AUTO_TERMINATED, // セッション自動終了
  DENY_INVITE,           // 招待受諾不可（体験版）

  // ここから下のエラー時は、リプレイセーブが行えない

  SIGNOUT_NETWORK,       // ネットワーク機能からサインアウト
  ACCEPT_INVITE_RESET,   // 招待を受諾したことによるリセット
  LOGOUT_RESET,          // アカウント情報変更によるリセット
  CHANGE_USER_REQ_RESET, // xone : ユーザ切り替え要求(PrimaryUser割り当てリセット)によるリセット

  ENUM_MAX_NUM,
};

const GAME_EXIT_RESULT_PLAY_RESTART = "restart";
const GAME_EXIT_RESULT_REPLAY_NEXT  = "rereplay";

enum SessionGameMode {
  NORMAL,
  RANKING,
  MULTIPLAY,
};

// 通常セッション検索 タイムアウト値
const SESSION_SEARCH_TIMEOUT = 5400;//90*60;	// frame


//----------------------------------------------------------------------
// NowLoading表示タイプ
//----------------------------------------------------------------------
enum NowLoadingTypes {
  NOW_LOADING,
  NOW_SAVEING,
  NOW_ACCESSING,
  PLEASE_WAIT,

  ENUM_MAX_NUM
};

//----------------------------------------------------------------------
// コンソールサイズの定数定義
//----------------------------------------------------------------------
// 禁則の表示に使う幅(ドット数)
const CONSOLE_PROHIBITION_WIDTH = 32;
// 通知ダイアログ（のコンソール）
//const NOTICE_DIALOG_SIZE_W = 992;//((30+1)*32)
//const NOTICE_DIALOG_SIZE_H = 400;
const NOTICE_DIALOG_SIZE_W = 992;//((30+1)*32)
const NOTICE_DIALOG_SIZE_H = 200;
// popupダイアログ（のコンソール）
const POPUP_DIALOG_SIZE_W = 512;//(15+1)*32
const POPUP_DIALOG_SIZE_H = 150;//(32+5)*4行

//----------------------------------------------------------------------
// ダイアログリソースの定数定義
//----------------------------------------------------------------------
// 縁パーツのサイズ
const DIALOG_EDGE_SIZE = 26;//(32)
// 中央パーツのサイズ
const DIALOG_CENTER_SIZE = 32;
// 基礎サイズ
const DIALOG_BASE_SIZE = 32;

//----------------------------------------------------------------------
// 通知ダイアログ
//----------------------------------------------------------------------
// 汎用のタイムアウト値（呼び元で使用）
const NOTICE_DIALOG_TIMEOUT = 180;//3*60;	// frame
// popupダイアログ表示期間の最低保証
const POPUP_DIALOG_WARRANTY_TIME = 360;//(6*60);	// frame
// 決定済みルール情報 ダイアログ表示期間の最低保証
const RULE_INFO_DIALOG_WARRANTY_TIME = 120;	// frame

enum LetsBuyDialogResult {
  TRY_PURCHASE,
  PURCHASED,
  DECIDE,
  CANCEL,
};

//----------------------------------------------------------------------
// 登録外字の文字コード
//----------------------------------------------------------------------
// ascii
const FONT_ASCII_BACK      = "σ ";	// <<
const FONT_ASCII_FORWARD   = " τ";	// >>
//const FONT_ASCII_CHOISE_UD = "γ ";	// 上下選択
//const FONT_ASCII_CHOISE_LR = "δ ";	// 左右選択
const FONT_ASCII_LIST_TOP  = "Ρ";	// 選択リスト終端(↑)
const FONT_ASCII_LIST_BTM  = "Υ";	// 選択リスト終端(↓)

// icon_code_arrayがFontIconCodeかどうかを識別するには、
// icon_code_array[0]が下記の文字列かで行う。
const FONT_ICON_CODE_0_TAG = "_FONT_";

// 以下の値はconfig/font_icon_code.incの値に合わせる
enum FontIconCodeIDs {
  _DUMMY_,
  CIRCLE,
  CROSS,
  TRIANGLE,
  SQUARE,
  L1,
  L2,
  L3,
  R1,
  R2,
  R3,
  SELECT,
  START,
  DIGITAL_U,
  DIGITAL_D,
  DIGITAL_L,
  DIGITAL_R,
  DIGITAL_UD,
  DIGITAL_LR,
  DIGITAL_ALL,
  SUB_CURSOR_L,
  SUB_CURSOR_R,
  SUB_CURSOR_U,
  SUB_CURSOR_D,

  CIRCLE__T,
  CROSS__T,
  TRIANGLE__T,
  SQUARE__T,
  L1__T,
  L2__T,
  L3__T,
  R1__T,
  R2__T,
  R3__T,
  SELECT__T,
  START__T,
  DIGITAL_U__T,
  DIGITAL_D__T,
  DIGITAL_L__T,
  DIGITAL_R__T,
  DIGITAL_UD__T,
  DIGITAL_LR__T,
  DIGITAL_ALL__T,
  SUB_CURSOR_L__T,
  SUB_CURSOR_R__T,
  SUB_CURSOR_U__T,
  SUB_CURSOR_D__T,
  COMMAND_DL_T,
  COMMAND_D__T,
  COMMAND_DR_T,
  COMMAND_L__T,
  COMMAND_R__T,
  COMMAND_UL_T,
  COMMAND_U__T,
  COMMAND_UR_T,
  COMMAND_A_T,
  COMMAND_B_T,
  COMMAND_C_T,
  COMMAND_D_T,
  ANTENNA_0_T,
  ANTENNA_1_T,
  ANTENNA_2_T,
  ANTENNA_3_T,
  ANTENNA_4_T,
  ANTENNA_5_T,
  NUM_MARU_1_T,
  NUM_MARU_2_T,
  NUM_MARU_3_T,
  NUM_MARU_4_T,
  NUM_MARU_5_T,
  NUM_MARU_6_T,
  NUM_MARU_7_T,
  NUM_MARU_8_T,
  NUM_MARU_9_T,
  NUM_MARU_10_T,
  NUM_ROME_1_T,
  NUM_ROME_2_T,
  NUM_ROME_3_T,
  NUM_ROME_4_T,
  NUM_ROME_5_T,
  NUM_ROME_6_T,
  NUM_ROME_7_T,
  NUM_ROME_8_T,
  NUM_ROME_9_T,
  NUM_ROME_10_T,
  NUM_MIC_T,
  NUM_MIC_MUTE_T,
  DIGITAL_UDR,
  X360_GUIDE_QUADRANTS_1,
  X360_GUIDE_QUADRANTS_2,
  X360_GUIDE_QUADRANTS_3,
  X360_GUIDE_QUADRANTS_4,
  X360_SHORT_SAVING_ICON,
  START_SMALL,
  CR_COPYRIGHT,
  CR_REGISTERED,
  LEADERBOARD_WITH_REPLAY,
  ANTENNA_X_T,
  AC_CREDIT_L,
  AC_CREDIT_M,
  AC_CREDIT_R,
  NUM_MIC_TALKING_T,

  ENUM_MAX_NUM
};

//----------------------------------------------------------------------
// メニューアイテム ステータス（汎用）
//----------------------------------------------------------------------
enum MenuItemStatus {
  ENABLE, 		// 有効(表示＆選択可＆操作可)
  DISABLE,		// 無効(非表示＆選択不可＆操作不可)
  GRAYOUT,		// グレーアウト(グレー表示＆選択可＆操作不可)

  ENUM_MAX_NUM
};

enum MotionShapeTracerFitMode {
  Y,
  X,
  FULL,
};

//----------------------------------------------------------------------
// SettingMenuFrame
//----------------------------------------------------------------------
// 
enum MenuFramePollResult {
  NONE,					// 変化無し
  DECIDE,				// 決定ボタンが押された
  //	CANCEL,			// キャンセルボタンが押された
  INDEX_CHANGED,		// index更新
  SUBITEM_CHANGED,		// subitem_index更新
  SECRET_OPENED,		// 隠しオープン
  MOVED_COORD,			// 座標移動

  ENUM_MAX_NUM
};

// サブアイテムform
enum SettingSubItemTypes {
  LEVEL,		// 数値			// [ level_min, level_max, unit_str ]
  STRINGS,	// 文字列リスト	// [ ... ]
  STRINGS_PARAGRAPH,	// 文字列リスト（タイトル＋選択項目が１行に入らない場合用に一段下に表示）	// [ ... ]

  ENUM_MAX_NUM

  INHERRITED_SUBITEM_CLASS = 0x000100,	// 独自実装subitem
};

// 自動画面サイズの種類
enum ScreenSizeAutoIndex { // setting_screen.json["AutoSize"]["subitem"]["form"]のvalueと合わせる
  NORMAL,
  ORIGINAL_ONLY_ONE,
  FIT,
  FULL,
  ORIGINAL,
  MANUAL,
};

enum RotateMode {
  NONE,
  ROT_L,
  ROT_R,
  ROT_180,
};

enum ShortcutFunc {
  NONE,
  MENU,
  RESET,
  UNDO,
  STATE_SAVE,
  STATE_LOAD,
};

// 中断データに保存するサムネイル情報(menu.inc内と合わせる)
const STATE_DATA_THUMBNAIL_WIDTH  =  320;
const STATE_DATA_THUMBNAIL_HEIGHT =  240;
const STATE_DATA_THUMBNAIL_DEPTH  =  3;


const PACKAGE_ITEMKEY_PACKED_LIST  = "dev_name_list"; // system_profのtitle_list中の複数パック定義キー

const TITLE_ITEMKEY_ON_TRIAL_VERSION = "on_trial_version";

const SETTING_DATAKEY_PRESET_TYPE = "_99_preset_type";

const MENU_ITEMKEY_DISABLE_ON_TRIAL_VERSION = "disable_on_trial_version";
const MENU_ITEMKEY_TEXT_LETS_BUY            = "text_lets_buy";

// 設定メニューで特殊扱いするキー
const SETTING_ITEMKEY_PRESET_TYPE = "PresetType";
const SETTING_ITEMKEY_TO_DEFAULT  = "ResetSettings";
const SETTING_ITEMKEY_EXIT        = "Exit";

// 設定メニューのプリセット設定種類
enum SettingPresetType {
  CUSTOM,
  DEFAULT,

  TITLE_BASE = 10,
};
