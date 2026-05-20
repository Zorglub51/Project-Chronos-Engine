//----------------------------------------------------------------------
// ユーティリティ関数定義
//----------------------------------------------------------------------

//----------------------------------------------------------------------
// メッセージarrayの文字列結合
// ・メッセージarray内の要素が1以上の数値（integer）の場合は
// 　iconコードindexとみなす。
// ・メッセージarray内の要素が0の数値（integer）の場合は
// 　code_0_stringとみなす。
// ・決定ボタンに相当するiconコードのswap機能付き
//----------------------------------------------------------------------
function join_message_array(msg_array, icon_code_array, code_0_string=null)
{
  local bCircleButtonIsDecide = SystemEtc.IsCircleButtonAssignedDecide();
  local bFontIcon = false;
  if (icon_code_array && icon_code_array[0] == FONT_ICON_CODE_0_TAG)
    bFontIcon = true;

  local join_str = "";

  foreach (val in msg_array) {
    local str;
    if ( (typeof val) == "integer" ) {
      if (icon_code_array && val > 0 && val < icon_code_array.len()) {
        if (bFontIcon && false == bCircleButtonIsDecide) {
          switch (val) {
          case FontIconCodeIDs.CIRCLE:	// circle_button icon
            val = FontIconCodeIDs.CROSS;
            break;
          case FontIconCodeIDs.CROSS:	// cross_button icon
            val = FontIconCodeIDs.CIRCLE;
            break;
          case FontIconCodeIDs.CIRCLE__T:	// circle_button icon
            val = FontIconCodeIDs.CROSS__T;
            break;
          case FontIconCodeIDs.CROSS__T:	// cross_button icon
            val = FontIconCodeIDs.CIRCLE__T;
            break;
          }
        }
        str = join_str + icon_code_array[val];
      }
      else if (val == 0 && code_0_string && code_0_string.len()) {
        str = join_str + code_0_string;
      }
      else {
        if (bFontIcon) 
          str = join_str;
        else 
          str = join_str + val.tostring();
      }
    }
    else {
      str = join_str + val;
    }
    join_str = str;
  }

  return join_str;
}

// ----

s_default_language_tag <- LANGUAGE_TAG_ENGLISH;

function set_item_string_default_language(_langTag)
{
  s_default_language_tag = _langTag;
  printf("set_item_string_default_language: %s\n", _langTag);
}

function get_item_string_default_language()
{
  return s_default_language_tag;
}

//----------------------------------------------------------------------
// 表示言語
//----------------------------------------------------------------------
s_debug_language_tag <- null;
s_language_tag <- null;	//koizumi add

//表示言語切替
function sys_set_language_tag(_tag)
{
  s_language_tag = _tag;
}

function sys_get_language_tag()
{
  if ( System.getDebugBuild() && (null != s_debug_language_tag) ) {
    return s_debug_language_tag;
  }
  local langTag = ::SystemEtc.getLanguageTag();
  
  if ( langTag == LANGUAGE_TAG_ENGLISH && s_default_language_tag == LANGUAGE_TAG_ENGLISH_UK ) {
    langTag = LANGUAGE_TAG_ENGLISH_UK; // 言語が英語かつパッケージのデフォルト言語がイギリス英語の場合は、イギリス英語を使う
  }
  
  if( s_language_tag != null )
  {
		//言語設定がある場合はそちらを採用する
		langTag = s_language_tag;
	}
  return langTag;
}

function debug_sys_set_language_tag(_tag)
{
  s_debug_language_tag = _tag;
}

//----------------------------------------------------------------------
// 文字列の格納方法を判別して、中の文字列を返す
// 		・テーブルの場合は多言語対応もしくはpackageRegion対応と見なす
// 		・配列の場合は文字列結合を行う
// 注：配列（文字列結合）内のテーブル（多言語対応）には未対応
//----------------------------------------------------------------------
function get_item_string(item, icon_code_array=null, code_0_string=null)
{
  if ((typeof item) == "string") {
    if ( regexp("MenuItemText__.*").match(item) ) {
      local table = ::_get_item_string_menu_item_text(item);
      return table ? ::get_item_string(table, icon_code_array, code_0_string) : item;
    }
    else if ( regexp("MenuInfoMsg__.*").match(item) ) {
      local table = ::_get_item_string_menu_info_msg(item);
      return table ? ::get_item_string(table, icon_code_array, code_0_string) : item;
    }
    else if ( regexp("NoticeMsg__.*").match(item) ) {
      local table = ::_get_item_string_notice_text(item);
      return table ? ::get_item_string(table, icon_code_array, code_0_string) : item;
    }
    else
      return item;
  }
  else if ((typeof item) == "table") {
    item = copy_PSBValue( item ); // XXX psbやstructの対応
    local bLang = (s_default_language_tag in item);
    if (bLang) {
      local languageTag = ::sys_get_language_tag();
      local str = (languageTag in item) ? item[ languageTag ] : item[ s_default_language_tag ];
      return ::get_item_string(str, icon_code_array, code_0_string);
    }
    else {
      local regionTag = ::get_package_regionTag();
      local str = (regionTag in item) ? item[ regionTag ] : item[ PACKAGE_REGION_USA ];
      return ::get_item_string(str, icon_code_array, code_0_string);
    }
  }
  else if ((typeof item) == "array") {
    local str = join_message_array(item, icon_code_array, code_0_string);
    return ::get_item_string(str, icon_code_array, code_0_string);
  }
  else if (null != item) {
    local str = item.tostring();
    return ::get_item_string(str, icon_code_array, code_0_string);
  }
  else
    return "";
}

function _get_item_string_menu_info_msg(_item)
{
  local menu_info_path = ::get_config_menu_info_path()
  return ::get_item_string_base(_item, menu_info_path, "message");
}

function _get_item_string_menu_item_text(_item)
{
  local menu_item_path = ::get_config_menu_item_path()
  return ::get_item_string_base(_item, menu_item_path, "item");
}

function _get_item_string_notice_text(_item)
{
  local notice_path = ::get_config_notice_path()
  return ::get_item_string_base(_item, notice_path, "notice");
}

function get_item_string_base(_item, _base_path, _menu_string_key)
{
  local str = null;
  local replace_path           = conv_path(CONFIG_MENU_STR_REPLACE_PATH);
  local pack_menu_string_path  = conv_path(CONFIG_TITLE_MENU_STRING_PATH, ::get_package_dev_id());
  local title_menu_string_path = conv_path(CONFIG_TITLE_MENU_STRING_PATH);
  local rsc = Resource();
  rsc.load(_base_path, replace_path, pack_menu_string_path, title_menu_string_path);
  while (rsc.loading)
    wait(0);

  local value = rsc.find(replace_path).root[_menu_string_key];
  if ( _item in value )
    _item = value[_item];

  local value_title = rsc.find(title_menu_string_path).root;
  local value_pack  = rsc.find(pack_menu_string_path).root;
  local value       = rsc.find(_base_path).root.message;
  if ( (_menu_string_key in value_title) && ( _item in value_title[_menu_string_key]) ) {
    str = copy_PSBValue( value_title[_menu_string_key][ _item ] );
  }
  else if ( (_menu_string_key in value_pack) && (_item in value_pack[_menu_string_key]) ) {
    str = copy_PSBValue( value_pack[_menu_string_key][ _item ] );
  }
  else if ( _item in value ) {
    str = copy_PSBValue( value[ _item ] );
  }
  else {
    str = "!" + _item; // キーが存在しなかった時の表示用
  }

  return str;
}

//----------------------------------------------------------------------
// 文字列中の日付指定記号を実際の日付けの数字に置き換え
function util_replace_time_string(_string, _date_info_table)
{
  local replace_table = [
                         { exp = "$yyyy", key = "year",   format = "%04d" },
                         { exp = "$mm",   key = "month",  format = "%02d" },
                         { exp = "$dd",   key = "day",    format = "%02d" },
                         { exp = "$HH",   key = "hour",   format = "%02d" },
                         { exp = "$MM",   key = "minute", format = "%02d" },
                         ];
  for (local no = 0; no < replace_table.len(); no++) {
    local index = null;
    local exp = replace_table[no]["exp"];
    while ( (index  = _string.find(exp)) != null ) {
      local before  = _string.slice(0, index);
      local after   = _string.slice(index + exp.len());
      local replace = format(replace_table[no]["format"], _date_info_table[replace_table[no]["key"]]);
      _string = before + replace  + after;
    }
  }
  return _string;
}

//----------------------------------------------------------------------
// ファイルパスからファイル名、パス、拡張子に分解して返す。
// 戻り値 : テーブル {
// 				path : path部分  (無ければ""、有れば終端に'/'を含む)
// 				name : name部分  (無ければ""、拡張子は最後のだけ取り除かれる)
// 				ext  : 拡張子部分(無ければ""、有れば先頭に'.'を含む)
// 			};
//----------------------------------------------------------------------
function fileparse(pathname)
{
  local ret = { path="", name="", ext="" };

  local last_at = function (str, sub) {
    local ret = null;
    local i = 0;
    do {
      i = str.find(sub, i);
      if (i != null) {
        i += sub.len();
        ret = i;
      }
    } while (i != null);
    return ret;
  }

  if (typeof(pathname) == "string") {
    local c0 = last_at(pathname, "/");
    local c1 = last_at(pathname, ".");

    if (c0 != null) 
      ret.path = pathname.slice(0, c0);
    else 
      c0 = 0;

    if (c1 != null) 
      ret.ext  = pathname.slice(--c1);
    else 
      c1 = pathname.len();

    ret.name = pathname.slice(c0, c1);
  }

  return ret;
}

//----------------------------------------------------------------------
// ScreenBoundsからV周波数を決定する
//----------------------------------------------------------------------
s_bInitFV <- false;
s_fFV <- null;

function init_frequency_V()
{
  local rect = System.getScreenBounds();
  local spec = System.getSpec();
  switch (spec) {
  case "psp":
    s_fFV = (60.0 * 1000.0) / 1001.0;
    break;

  case "ps3":
    if (rect.height == 576) 	// 720x576
      {
        //			s_fFV = 50.0;
        s_fFV = (60.0 * 1000.0) / 1001.0;
      }
    else
      {
        s_fFV = (60.0 * 1000.0) / 1001.0;
      }
    break;

  default:
    s_fFV = (60.0 * 1000.0) / 1001.0;
    break;
  }
	
  s_bInitFV = true;
}

function get_frequency_V()
{
  if (!s_bInitFV) 
    init_frequency_V();

  return s_fFV;
}

//----------------------------------------------------------------------
// ScreenBoundsからlayerの倍率を決定する
// （将来的には廃止）
//----------------------------------------------------------------------

function set_layer_scale(_layer)
{
  local rect = System.getScreenBounds();
  local scale = rect.height / SCREEN_YSIZE;

  _layer.setZoom(scale);
  if (1.0 != scale) {
    _layer.smoothing = true;
  }

  return scale; // 呼び元での再計算＆設定用に倍率値を返す
}

// ----

s_session_dialog_layerFolder <- null;
s_backup_control_layerFolder <- null;
s_menu_normal_layerFolder <- null;
s_game_screen_layerFolder <- null;
s_menu_bg_layerFolder <- null;


function util_create_layerFolders(_display = null)
{
  s_session_dialog_layerFolder = ScaledLayerFolder(_display, PRIORITY_LAYER_FOLDER_MENU_NOTIFY);
  s_backup_control_layerFolder = ScaledLayerFolder(_display, PRIORITY_LAYER_FOLDER_MENU_BACKUP);
  s_menu_normal_layerFolder    = ScaledLayerFolder(_display);
  s_game_screen_layerFolder    = ScaledLayerFolder(_display, PRIORITY_LAYER_FOLDER_GAME_SCREEN);
  s_menu_bg_layerFolder        = ScaledLayerFolder(_display, PRIORITY_LAYER_FOLDER_WALLPAPER);
}

function util_get_session_dialog_layerFolder()
{
  return s_session_dialog_layerFolder;
}

function util_get_backup_control_layerFolder()
{
  return s_backup_control_layerFolder;
}

function util_get_menu_layerFolder()
{
  return s_menu_normal_layerFolder;
}

function util_get_game_screen_layerFolder()
{
  return s_game_screen_layerFolder;
}

function util_get_menu_bg_layerFolder()
{
  return s_menu_bg_layerFolder;
}

class ScaledLayerFolder extends LayerFolder {

  rotate_mode = null;

  constructor(_owner = null, _priority = null) {
    ::LayerFolder.constructor(_owner);

    rotate_mode = RotateMode.NONE;
    ::LayerFolder.setAngleDeg( _calc_deg(rotate_mode) );
    ::LayerFolder.setPriority( _priority != null ? _priority : PRIORITY_LAYER_FOLDER_MENU_NORMAL );
  }

  function _calc_deg(_mode) {
    local deg = 0;
    switch (_mode) {
    case RotateMode.NONE:    deg =   0; break;
    case RotateMode.ROT_L:   deg = -90; break;
    case RotateMode.ROT_R:   deg =  90; break;
    case RotateMode.ROT_180: deg = 180; break;
    }
    return deg;
  }
  function _calc_scale(_mode) {
    local scale = 0.7;// XXX
    switch (_mode) {
    case RotateMode.NONE:    scale = 1.0; break;
    case RotateMode.ROT_L:                break;
    case RotateMode.ROT_R:                break;
    case RotateMode.ROT_180: scale = 1.0; break;
    }
    return scale;
  }


  function set_rotate_mode(_mode) {
    if (rotate_mode == _mode) {
      return;
    }
    rotate_mode = _mode;

    local deg   = _calc_deg(_mode);
    local scale = _calc_scale(_mode);
    
    ::LayerFolder.setAngleDeg( deg );
    ::LayerFolder.setZoom( scale );
  }

};

class ScaledLayer extends LayerFit {

  constructor(owner = null) {
    ::LayerFit.constructor( owner ? owner : ::util_get_menu_layerFolder() );

    ::LayerFit.setFitHeight( SCREEN_YSIZE );
  }

  function registerMotionResource(_motion_psb, _purge_psb = null) {
    if ( null != _purge_psb && "ps3" == System.getSpec() )
      _motion_psb.preparePurge();

    local resourceId = ::Layer.registerMotionResource(_motion_psb);

    if ( null != _purge_psb && "ps3" == System.getSpec() )
      _motion_psb.purgeStreamEntity();

    return resourceId;
  }

  function clip_coord_by_safe_area(_coord_x, _coord_y, _check_bounds) {
    local safe_bounds = SystemEtc.getSafeScreenBounds();
    local scale = this.getFitScale();
    {
      // XXX （将来的には廃止）D5スケールでの座標に変換
      safe_bounds.left   /= scale;
      safe_bounds.right  /= scale;
      safe_bounds.top    /= scale;
      safe_bounds.bottom /= scale;
    }

    if (_coord_x + _check_bounds.left < safe_bounds.left)
      _coord_x = safe_bounds.left - _check_bounds.left;
    if (_coord_x + _check_bounds.right > safe_bounds.right)
      _coord_x = safe_bounds.right - _check_bounds.right;

    if (_coord_y + _check_bounds.top < safe_bounds.top)
      _coord_y = safe_bounds.top - _check_bounds.top;
    if (_coord_y + _check_bounds.bottom > safe_bounds.bottom) {
      _coord_y = safe_bounds.bottom - _check_bounds.bottom;
    }

    return {x = _coord_x, y = _coord_y};
  }

};

// ----

// リリース時キャッシュするリソース用
class ResourceCache {
  m_rsc = null;
  m_path = null;

  constructor() {
    m_rsc = null;
    m_path = [];
  }
  
  function init(_path) {
    local path;
    if ( "array" == typeof(_path) ) {
      path = _path;
    }
    else {
      path = [];
      path.append(_path);
    }

    local no; 
    for (no = 0; no < path.len() && no < m_path.len(); no++) {
      if (path[no] != m_path[no])
        break;
    }
    if (no != path.len()) {
      m_path = path;
      if (null == m_rsc)
        m_rsc = Resource();
      else
        m_rsc.unload();

      m_rsc.load(m_path);
      while (m_rsc.loading)
        wait(0);
    }
  } 

  function get(_path) {
    return m_rsc.find(_path);
  }

};

// ----

// 指定サイズ以上になったら幅を縮小して指定幅に収まるように表示するインジケータ
class IndicatorFitMaxWidth extends Indicator {
  
  m_max_width = null;
  m_text = "";

  constructor(_layer, _font, _max_width = 0) {
    m_max_width = _max_width;
    ::Indicator.constructor(_layer, _font);
  }

  function print(_text) {
    m_text = _text;
    ::Indicator.print(m_text);
    if (m_max_width != 0 && m_max_width < this.width) {
      local old_scale = ::Indicator.getFontScaleX();
      ::Indicator.setFontScaleX( old_scale * m_max_width / this.width );
      ::Indicator.print(m_text);
    }
  }

  function setFontScaleX(_scaleX) {
    ::Indicator.setFontScaleX( _scaleX );
    this.print( m_text );
  }

};

// ----

class PauseMenuControlRegion {
  
  m_latency_region = null;
  m_screen_saver = null;
  
  constructor() {
    m_latency_region = DrawLatencyInPlayerControlRegion(DrawLatencyInPlayerControlRegion.NOT_PLAYER_CONTROL);
    m_screen_saver   = EnableScreenSaverRegion(true);
  }

};

// ----

class DrawLatencyInPlayerControlRegion extends Object {

  m_last = null;
  static NOT_PLAYER_CONTROL = true;

  constructor(_not_player_control = false) {
    ::Object.constructor();

    m_last = System.getDrawLatency();
    local disable_1v = ::util_is_disable_1v();
    local latency = (_not_player_control || disable_1v) ? SystemDrawLatency.MODE_2V : SystemDrawLatency.MODE_1V;
    if ( ::util_is_use_3dmode() ) {
      latency = SystemDrawLatency.MODE_2V; // XXX 現状360で60でまわらないので
    }
    System.setDrawLatency( latency );
    printf("\n\nSet to Latency %s->%s\n\n", m_last, latency);
  }

  function destructor() {
    System.setDrawLatency( m_last );
    printf("\n\nReturn to Latency %s\n\n", m_last);
  }

};

// ----

class EnableScreenSaverRegion extends Object {

  m_last = null;

  constructor(_enable) {
    ::Object.constructor();

    m_last = SystemEtc.getEnableScreenSaver();
    SystemEtc.setEnableScreenSaver( _enable );
  }

  function destructor() {
    SystemEtc.setEnableScreenSaver(m_last);
  }

};

//----------------------------------------------------------------------
// PSBValueの完全なコピーを生成する
//----------------------------------------------------------------------
function _recursive_copy_PSBValue(dst, srcValue)
{
  switch (typeof srcValue) {
  case "null":
  case "integer":
  case "float":
  case "bool":
    dst = srcValue;
  break;

  case "string":
    dst = srcValue.slice(0);
    break;

  case "array":
    dst = [];
    foreach (val in srcValue) {
      dst.append( _recursive_copy_PSBValue(null, val) );
    }
    break;

  case "table":
    dst = {};
    foreach (key, val in srcValue) {
      dst[ key ] <- _recursive_copy_PSBValue(null, val);
    }
    break;
  }
	
  return dst;
}

function copy_PSBValue(srcValue)
{
  local copied_table = _recursive_copy_PSBValue(null, srcValue);
  return copied_table;
}

//----------------------------------------------------------------------
// テーブルから同一構造のStructへ各フィールド値を代入する
//----------------------------------------------------------------------
function setField_Table2Struct(_dstStruct, _srcTable)
{
  _recursive_setField_Table2Struct(_dstStruct.root, _srcTable)
}

function _recursive_setField_Table2Struct(_dst, _src)
{
  switch (typeof _src) {
  case "array":
    // Structの配列は全要素同一型
    switch (typeof _src[ 0 ]) {
    case "array":
    case "table":
      local src_len = _src.len();
      local dst_len = _dst.len();
      for (local index = 0; index < src_len && index < dst_len; index++) {
        _recursive_setField_Table2Struct(_dst[ index ], _src[ index ]);
      }
      break;

    default:
      local src_len = _src.len();
      local dst_len = _dst.len();
      for (local index = 0; index < src_len && index < dst_len; index++) {
        _dst[ index ] = _src[ index ];
      }
      break;
    }
    break;

  case "table":
    foreach (key, val in _src) {
      switch (typeof val) {
      case "array":
      case "table":
        _recursive_setField_Table2Struct(_dst[ key ], val);
        break;

      default:
        _dst[ key ] = val;
        break;
      }
    }
    break;

  default:
    printf("\n\t_recursive_setField_Table2Struct: Illegal struct src=%s typeof src=%s dst=%s\n\n", _src, typeof _src, _dst);
    break;
  }
	
  return _dst;
}

//----------------------------------------------------------------------
// Structから同一構造を持つTableを生成する
//----------------------------------------------------------------------
function copy_Struct2Table(_srcStruct)
{
  local srcValue = _srcStruct.getRoot();
  return _recursive_copy_Struct2Table(srcValue)
}

function _recursive_copy_Struct2Table(_src)
{
  local dst = null;
  switch (typeof _src) {
  case "array":
    // Structの配列は全要素同一型
    dst = [];
    switch (typeof _src[ 0 ]) {
    case "array":
    case "object":
      local src_len = _src.len();
      for (local index = 0; index < src_len; index++) {
        dst.append( _recursive_copy_Struct2Table(_src[ index ]) );
      }
      break;

    default:
      local src_len = _src.len();
      for (local index = 0; index < src_len; index++) {
        dst.append( _src[ index ] );
      }
      break;
    }
    break;

  case "object":
    dst = {};
    foreach (key, val in _src) {
      switch (typeof val) {
      case "array":
      case "object":
        dst[ key ] <- _recursive_copy_Struct2Table(val);
        break;

      default:
        dst[ key ] <- val;
        break;
      }
    }
    break;

  default:
    printf("\n\t_recursive_copy_Struct2Table: Illegal struct src=%s typeof src=%s dst=%s\n\n", _src, typeof _src, dst);
    break;
  }
	
  return dst;
}

//----------------------------------------------------------------------
// _targetテーブルに_srcをテーブルを上書きマージする
//----------------------------------------------------------------------
function merge_tables(_src, _target)
{
  // printf("merge_tables: src=%s typeof src=%s _target=%s typeof _target=%s\n", _src, typeof _src, _target, typeof _target);
  switch (typeof _src) {
  case "array":
    if ( (null == _target) || (typeof _target != "array") ) 
      _target = [];
    local src_len = _src.len();
    for (local index = 0; index < src_len; index++) {
      switch (typeof _src[ 0 ]) {
      case "array":
      case "table":
        if (_target.len() > index)
          _target[index] = merge_tables(_src[ index ], _target[index]);
        else
          _target.append( merge_tables(_src[ index ], null) );
      break;

      default:
        // printf("\tmerge_tables: index=%10d val=%s\n", index, _src[index]);
        if (_target.len() > index)
          _target[index] = _src[index];
        else
          _target.append( _src[ index ] );
        break;
      }
    }
    break;

  case "table":
    if ( (null == _target) || (typeof _target != "table") ) 
      _target = {};
    foreach (key, val in _src) {
      switch (typeof val) {
      case "array":
      case "table":
        if (key in _target) 
          _target[ key ] = merge_tables(val, _target[ key ]);
        else
          _target[ key ] <- merge_tables(val, null)
        break;

      default:
        // printf("\tmerge_tables: key=%-20s val=%-20s\n", key, val);
        if (key in _target) 
          _target[ key ] = val;
        else
          _target[ key ] <- val;
        break;
      }
    }
    break;

  default:
    printf("\n\tmerge_tables: Illegal struct src=%s typeof src=%s _target=%s\n\n", _src, typeof _src, _target);
    break;
  }
	
  return _target;
}

//----------------------------------------------------------------------
// リプレイ再生 リセット
//----------------------------------------------------------------------
function reset_play_replay(_is_rankingattack = false)
{
  // リプレイ再生 リセット
  ::g_emu_task.setPlayReplayMode(PlayReplayMode.RESET);

  // PlayReplayMode.RESETが未サポート？
  if ( ::g_emu_task.getPlayReplayMode() != PlayReplayMode.RESET) {
    // リプレイ再生 開始
    ::start_replay_play( _is_rankingattack );
  }
  else {
    // PlayReplayMode.RESETがサポートされてるなら、あとは勝手に
    // リセット後にPlayReplayMode.PLAY_REPLAY_PLAYへ移行する
  }

  if ( ::g_replay_control ) {
    ::g_replay_control.reseted_play_replay();
  }

  local state_check = ::g_emu_task.getStateCheckControl();
  if ( null != state_check ) {
    state_check.init_status(::g_emu_task, ::g_systemdata);
  }
}

//----------------------------------------------------------------------
// リプレイ再生が終わったか
//----------------------------------------------------------------------
function is_replay_complete(_emu_task)
{
  return _emu_task.replayComplete && (
                                      (_emu_task.replayMode == "record") ||
                                      (_emu_task.replayMode == "play" && (_emu_task.getPlayReplayMode() != PlayReplayMode.PAUSE))
                                      );
}

//----------------------------------------------------------------------
// 名前で設定、検索を行うキーリスト
//	動的な変数として使える
//----------------------------------------------------------------------
listItemKey <- null;
listItemKeyMode <- true;

class listItemKeyData
{
	name = null;
	key  = null;
	init = null;

	constructor(_name, _item)
	{
		name = _name;
		key  = _item;
		init = _item;
	}
};

//記録、検索の有無を設定
function modetItemKey(_mode)
{
	listItemKeyMode = _mode
}

//名称をつけてキーを設定
function setItemKey(name, itemKey)
{
	if( listItemKeyMode == false )
	{
		return false;
	}

	if( listItemKey == null )
	{
		listItemKey = [];
	}
	else
	{
		//登録内に見つかったらキーを変更
		foreach(item in listItemKey)
		{
			if( item.name == name )
			{
				item.key = itemKey;
				return true;
			}
		}
	}

	listItemKey.append
	(
		listItemKeyData
		(
			name,
			itemKey
		)
	);

	return false;
}

//名称からキーを検索
//（みつからない場合は、初期値を設定して返す）
function getItemKey(name, initKey=0)
{
	if( listItemKeyMode == false )
	{
		return initKey;
	}

	if( listItemKey == null )
	{
		listItemKey = [];
	}
	else
	{
		//登録量が多くなった場合の検索速度が心配。（毎サイクル呼ぶような処理には適していない）
		foreach(item in listItemKey)
		{
			if( item.name == name )
			{

				return item.key;
			}
		}
	}

	listItemKey.append
	(
		listItemKeyData
		(
			name,
			initKey
		)
	);

	return initKey;
}

//登録されているキーを削除する
//	何か削除できたらtrueを返す
function delItemKey(name=null, itemKey=null)
{
	if( listItemKey == null )
	{
		return false;
	}

	//名称で検索して削除
	if( name )
	{
		foreach(item in listItemKey)
		{
			if( item.name == name )
			{
				listItemKey.remove(item);
				return true;
			}
		}
	}
	//キーで検索して見つかったもの全てを削除
	else
	if( itemKey )
	{
		local result = false;
		foreach(item in listItemKey)
		{
			if( item.key == itemKey )
			{
				result = true;
				listItemKey.remove(item);
			}
		}

		return result;
	}
	//全削除
	else
	{
		listItemKey = null;

		return true;
	}

	return false;
}

//初期値に戻す(initKey設定時は初期値を変えて設定)
function initItemKey(name, initKey=null )
{
	if( listItemKey == null )
	{
		return false;
	}

	{
		foreach(item in listItemKey)
		{
			if( item.name == name )
			{
				//再設定
				if( initKey )
				{
					item.init = initKey;
				}

				item.key = item.init

				return true;
			}
		}
	}

	return false;
}

//全てのキーを初期値に戻す
function initallItemKey()
{
	if( listItemKey == null )
	{
		return false;
	}

	{
		foreach(item in listItemKey)
		{
			item.key = item.init
			printf( ">%s	%s\n", item.name, item.key );
		}
	}

	return true;
}

//----------------------------------------------------------------------
// １６進数の文字列を数値に変換
//----------------------------------------------------------------------
function hexstr_tointeger(str)
{
	local num = 0;				// 結果
	local mul = 1;				// 桁ごとの基本値

	//最後の文字から順番に変換
	for( local i = (str.len() - 1) ; i >= 0 ; i-- )
	{
		local tgt = str[i];

		switch( tgt )
		{
		case '0':
		case '1':
		case '2':
		case '3':
		case '4':
		case '5':
		case '6':
		case '7':
		case '8':
		case '9':
			num += (tgt - '0'     ) * mul;
		break;
		case 'a':
		case 'b':
		case 'c':
		case 'd':
		case 'e':
		case 'f':
			num += (tgt - 'a' + 10) * mul;
		break;
		case 'A':
		case 'B':
		case 'C':
		case 'D':
		case 'E':
		case 'F':
			num += (tgt - 'A' + 10) * mul;
		break;
		//対象外の文字は無視する
		default:
			continue;
		break;
		}

		//桁上がり
		mul *= 16;
	}

	return num;
}


//----------------------------------------------------------------------
// 文字列中の半角スペースを全角スペースに変換
//----------------------------------------------------------------------
function string_half_space_to_wide_space(_str)
{
  local ret_str = "";
  for (local no = 0; no < _str.len(); no++) {
    local letter = _str.slice(no, no+1);
    ret_str += (" " == letter) ? "　" : letter;
  }

  return ret_str;
}

//----------------------------------------------------------------------
// SD4:3解像度モードか
//----------------------------------------------------------------------
function is_screen_mode_sd4_3()
{
  return is_screen_mode_sd() && !is_screen_mode_wide();
}

//----------------------------------------------------------------------
// SD解像度モードか
//----------------------------------------------------------------------
function is_screen_mode_sd()
{
  return System.getScreenBounds().height <= 576.0;
}

//----------------------------------------------------------------------
// WIDE解像度か
//----------------------------------------------------------------------
function is_screen_mode_wide()
{
  return System.getScreenBounds().height / System.getScreenBounds().width <= 0.7;
} 

function get_scale_value_to_fit_wide()
{
  local width = SCREEN_YSIZE * System.getScreenBounds().width / System.getScreenBounds().height;
  return width / SCREEN_XSIZE;
}


// 外部参照で複数に分かれている可能性のあるモーションファイルを登録する
function register_multi_motion_to_layer(_layer, _config, _key, _default_path = null, _purge = null, _ignore_language = false)
{
  local key_base = _key + "_base";
  local paths = ::get_multi_motion_paths(_config, _key, _default_path, _ignore_language);
  local rsc = Resource();
  rsc.load(paths);
  while (rsc.loading) {
    wait(0);
  }
  for (local no = 0; no < paths.len(); no++) {
    _layer.registerMotionResource( rsc.find(paths[no], _purge) );
  }
}

// 外部参照で複数に分かれている可能性のあるモーションファイルのファイルリストを取得する
function get_multi_motion_paths(_config, _key, _default_path = null, _ignore_language = false)
{
  local key_base = _key + "_base";
  local motion_base_path = key_base in _config ? ::conv_path( ::get_motion_path_by_environment(_config[key_base]) ) : null;
  local motion_lang_path = _key     in _config ? ::conv_path( ::get_motion_path_by_environment( _config[_key]   ) ) : null;
  local lists = [];
  if (motion_base_path) {
    lists.append(motion_base_path);
  }
  if (motion_lang_path) {
    lists.append(motion_lang_path);
  }
  else if (_default_path) {
    lists.append(_default_path);
  }
  
  return lists;
}

function get_motion_path_by_environment(_motion_path, _ignore_language = false)
{
  if (_ignore_language || (typeof _motion_path == "string")) { 
    return "motion/" + _motion_path + ".psb";
  }
  else {
    return "motion/" + ::get_item_string(_motion_path) + ".psb";
  }
}

//----------------------------------------------------------------------
// replay_structにロード済みのリプレイを再生する
//----------------------------------------------------------------------
function play_replay_on_replay_struct(_from_leaderboard = false)
{
  local result = null;
  local replay_struct = ::get_backup_struct(BackupSegmentTypes.EMU_REPLAY);
  local replay_mode   = replay_struct.root._01_added_value._07_mode;

  if ( ::isDebugBuild() ) {
    if ( ::g_input.key(KEY.L) ) {
      ::host_save_emulator_replaydata(true);
    }
    else if ( ::g_input.key(KEY.L2) ) {
      ::host_load_emulator_replaydata();
    }
    else if ( ::g_input.key(KEY.R) ) {
      switch (replay_mode) {
      case BackupGameMode.RANKING:
        ::util_load_script( conv_path(SCRIPT_PLAY_RANKINGATTACK_PATH) );
        ::save_replay_data(true);
        break;

      default:
        create_savedata_capture(); // セーブデータicon用 emu画面キャプチャ
        ::save_replay_data(true);
        release_savedata_capture(); // セーブデータicon用 emu画面キャプチャ解放
        break;
      }
    }
  }

  local last_od = ::g_emu_task.overdrive;
  ::g_emu_task.overdrive = 0; // リプレイ再生中はoverdriveを適用しない
  switch (replay_mode) {
  case BackupGameMode.RANKING:
    result = ::_play_rankingattack_replay(_from_leaderboard);
    break;

  default:
    ::util_load_script( conv_path(SCRIPT_PLAY_STANDALONE_PATH) );
    local disable_sns = replay_struct.root._01_added_value._07_bitflag & BackupReplaydataBitFlag.DISABLE_SNS;
    result = ::play_standalone(true, replay_mode, disable_sns);
    break;
  }
  ::g_emu_task.overdrive = last_od;
  return result;
}

//----------------------------------------------------------------------
// replay_structにロード済みのトライアルのリプレイを再生する
//----------------------------------------------------------------------
function _play_rankingattack_replay(_from_leaderboard = false)
{
  local rsc = Resource();
  local replay_struct_path = conv_path(STRUCT_REPLAYDATA_PATH);
  rsc.load(replay_struct_path);
  while (rsc.loading)
    wait(0);

  local game_exit = false;
  local replay_struct_src = ::get_backup_struct(BackupSegmentTypes.EMU_REPLAY);
  local replay_struct_dst = Struct( rsc.find(replay_struct_path) );
  if ( BinaryUtil.Struct2Struct(replay_struct_dst, replay_struct_src) && ::set_replay_data_to_emu(replay_struct_src) ) {
    ::util_load_script( conv_path(SCRIPT_PLAY_RANKINGATTACK_PATH) );
    ::util_load_script( conv_path(SCRIPT_PLAY_REPLAY_PATH) );
    game_exit = ::play_rankingattack(::get_rankingattack_kind_from_savedata( replay_struct_src.root._01_added_value._08_submode ),
                                     true,
                                     replay_struct_dst,
                                     _from_leaderboard,
                                     replay_struct_src.root._01_added_value._07_bitflag & BackupReplaydataBitFlag.DISABLE_SNS
                                     );
  }
  return game_exit;
}

//----------------------------------------------------------------------
// ロードしたconfigをキャッシュして取得するためのラッパー
s_loaded_config <- {};

function util_load_psb_get_by_instance(_path, _only_cache_when_release_build = false)
{
  local force_reload = _only_cache_when_release_build && ::isDebugBuild(); // 読み替えると最後の参照しか生きていないので、instanceで使う時は読み替えず常にキャッシュ
  // FOLDER HACK: bypass the PSB cache so config files swapped by the
  // ::enterGameFolder() / ::exitGameFolder() natives are picked up on next read.
  force_reload = true;
  if ( !(_path in s_loaded_config) || force_reload ) {
    s_loaded_config[_path] <- ResourceCache();
  }
  s_loaded_config[_path].init(_path);
  return s_loaded_config[_path].get(_path);
}

function util_load_config(_path)
{
  return copy_PSBValue( ::util_load_psb_get_by_instance(_path, true).root );
}

//----------------------------------------------------------------------
// 実際にゲームするプレイヤー人数
function util_get_player_real_num(_title_prof = null, _gameRegionTag = null)
{
  local title_prof = (null == _title_prof) ? ::get_current_title_prof() : _title_prof;
  local gameRegionTag = (null == _gameRegionTag) ? ::g_systemdata.get_data_setting_etc__game_regionTag() : _gameRegionTag;

  if ( "player_num" in title_prof.m2epi.version[gameRegionTag] ) {
    return title_prof.m2epi.version[gameRegionTag].player_num;
  }
  else {
    return title_prof.player_num;
  }
}

// 有効なコントローラ数(通常はプレイヤー人数に一致)
function util_get_player_input_num(_title_prof = null, _gameRegionTag = null)
{
  local title_prof = (null == _title_prof) ? ::get_current_title_prof() : _title_prof;
  local gameRegionTag = (null == _gameRegionTag) ? ::g_systemdata.get_data_setting_etc__game_regionTag() : _gameRegionTag;

  if ( "player_input_num" in title_prof.m2epi.version[gameRegionTag] ) {
    return title_prof.m2epi.version[gameRegionTag].player_input_num;
  }
  else if ( "player_input_num" in title_prof)
    return title_prof.player_input_num;
  else
    return ::util_get_player_real_num(title_prof, _gameRegionTag);
}

// 連射速度テーブル取得
function util_get_controller_rapid_speed_list(_title_prof = null, _gameRegionTag = null)
{
  local title_prof = (null == _title_prof) ? ::get_current_title_prof() : _title_prof;
  local gameRegionTag = (null == _gameRegionTag) ? ::g_systemdata.get_data_setting_etc__game_regionTag() : _gameRegionTag;

  if ( "rapid_speed_list" in title_prof.m2epi.version[gameRegionTag] ) {
    return title_prof.m2epi.version[gameRegionTag]["rapid_speed_list"];
  }
  else {
    return title_prof.controller.rapid_speed_list;
  }
}

// ゲーム設定がある(主にアーケード)タイトルか
function util_is_use_gamesettings()
{
  local title_prof = ::get_current_title_prof();
  if ( "use_gamesettings" in title_prof && title_prof.use_gamesettings )
    return true;
  else 
    return false;
}

// 立体視3D表示に対応したタイトルか
function util_is_use_3dmode(_title_prof = null)
{
  local title_prof = (null == _title_prof) ? ::get_current_title_prof() : _title_prof;
  if ( "use_3dmode" in title_prof && title_prof.use_3dmode )
    return true;
  else 
    return false;
}

// ネットワークマルチプレイに対応したタイトルか
function util_is_use_multiplay()
{
  local title_prof = ::get_current_title_prof();

  if ( "use_multiplay" in title_prof && title_prof.use_multiplay )
    return true;
  else 
    return false;
}

// SRAMクリア(プレイデータリセット)に対応したタイトルか
function util_is_use_sram_clear()
{
  local title_prof = ::get_current_title_prof();

  if ( "use_sram_clear" in title_prof && title_prof.use_sram_clear )
    return true;
  else 
    return false;
}

// 1Vを許可しないタイトルか
function util_is_disable_1v()
{
  local title_prof = ::get_current_title_prof();

  if ( "disable_1v" in title_prof && title_prof.disable_1v )
    return true;
  else 
    return false;
}

// 画面モードファインの正数倍方向を逆転させるか
function util_is_screen_fine_reverse()
{
  local title_prof = ::get_current_title_prof();
  if ( "screen_fine_reverse" in title_prof && title_prof.screen_fine_reverse )
    return true;
  else 
    return false;
}

// 画面回転強制有効か
function util_is_screen_rotate_force_enable()
{
  local title_prof = ::get_current_title_prof();
  if ( "screen_rotate_force_enable" in title_prof && title_prof["screen_rotate_force_enable"] )
    return true;
  else 
    return false;
}

// 実績に対応したタイトルか
function util_is_medal_system_support()
{
  if ( !SystemEtc.isSupportMedalSystem() ) {
    return false; // プラットフォーム的に非対応
  }
  
  local title_prof_package = ::get_package_title_prof();
  local title_prof_current = ::get_current_title_prof();
  if ( "medal_system_support" in title_prof_package ) {
    return title_prof_package["medal_system_support"];
  }
  else if ( "medal_system_support" in title_prof_current ) {
    return title_prof_current["medal_system_support"];
  }
  else {
    return false;
  }
}

// ステートロード時にsramを反映させないタイトルか
function util_is_no_update_sram_when_state_load()
{
  local title_prof = get_current_title_prof();

  if ( "sram_noupdate_when_state_load" in title_prof.m2epi && title_prof.m2epi.sram_noupdate_when_state_load )
    return true;
  else
    return false;
}

// ステートロード時のシステム状態変更チェックでSRAMの変更を考慮しないタイトルか
function util_is_no_check_sram_when_state_load()
{
  local title_prof = get_current_title_prof();

  if ( "sram_nocheck_when_state_load" in title_prof.m2epi && title_prof.m2epi.sram_nocheck_when_state_load )
    return true;
  else
    return false;
}

// バックアップメモリが存在するか
function util_is_sram_support(_gameRegionTag = null)
{
  local title_prof = get_current_title_prof();
  local gameRegionTag = (null == _gameRegionTag) ? ::g_systemdata.get_data_setting_etc__game_regionTag() : _gameRegionTag;

  if ( "sram_support" in title_prof.m2epi.version[gameRegionTag] ){  
    return title_prof.m2epi.version[gameRegionTag].sram_support;
  }
  else if ( "sram_support" in title_prof.m2epi ) {
    return title_prof.m2epi.sram_support;
  }
  else {
    return false;
  }
}

// 起動時デバッグ用ログ出力ありのタイトルか
function util_is_debug_logging_at_start()
{
  local title_prof = get_current_title_prof();

  if ( "debug_logging" in title_prof.m2epi && title_prof.m2epi.debug_logging )
    return true;
  else
    return false;
}

// IDダブ割当許可タイトルか
function util_is_enable_same_pad_id_assign()
{
  local title_prof = ::get_current_title_prof();

  if ( "enable_same_id_assign" in title_prof.controller && title_prof.controller.enable_same_id_assign )
    return true;
  else
    return false;
}

// --

// タイトル画面ありか
function util_is_exist_title_screen()
{
  return (
          ::is_package_containing_multi_title() // 複数タイトルまとめパックはタイトル画面あり
          || System.getSpec() == "x360"         // 360はサインインの関係で必ずタイトル画面あり
          || ::is_package_force_use_title_screen() // 明示的にタイトル画面ありとする
          );
}

// エミュレータの3Dモードからシステムの3Dモードを設定する
function util_set_system_3d_mode(_m2epi_3dmode) {
  switch (_m2epi_3dmode) {
  case M2Epi3DMode.FRAMEPACKING:
    System.setStereo3DMode(1);
    break;

  case M2Epi3DMode.SIDEBYSIDE:
    System.setStereo3DMode(2); // real side by side
    break;

  default:
    System.setStereo3DMode(0);
    break;
  }
}

// エミュレータの3Dモードからシステムの3Dが必要か判定
function util_is_need_system_3d(_m2epi_3dmode) {
  if (M2Epi3DMode.FRAMEPACKING == _m2epi_3dmode)
    return true;
  else 
    return false;
}

// ----

// ボタンを押したPadのidを取得する
function util_get_start_pad_id(_key)
{
  local start_pad_bits = ::g_input.padOnKeyPressed(_key, false);
  local start_player_no = 0;
  local padNum = ::g_inputHub.inputNum;  // コントローラID数
  local start_pad_id = 0;
  for (local pad_id = 0; pad_id < padNum; pad_id++) {
    if ( (1 << pad_id) & start_pad_bits ) {
      start_pad_id = pad_id;
      break;
    }
  }
  printf("key=0x%08x: start_pad_bits=0x%08x, connected=0x%08x, inputNum = %d -> start_pad_id = %d\n", _key, start_pad_bits, ::g_inputHub.connected, padNum, start_pad_id);
  return start_pad_id;
}

// ボタンを押したPadに割り当てられているプレイヤーを取得する(割り当てがなければnull)
function util_get_start_player_no(_key)
{
  local start_pad_id    = ::util_get_start_pad_id(_key);
  local paramLists_IDs  = ::g_systemdata.get_value(SystemDataValueIndex.SETTING_PAD).get_IDs();
  local start_player_no = null;
  for (local player_no = 0; player_no < ::util_get_player_real_num(); player_no++) {
    if (paramLists_IDs[player_no] == start_pad_id) {
      start_player_no = player_no;
      break;
    }
  }
  return start_player_no;
}

// 完全版購入
function util_buy_full_game(_pad_id)
{
  ::check_login(_pad_id);

  local dev_id = ::get_package_dev_id();
  local title_item = get_title_item(dev_id);

  if ( SystemEtc.buyFullGame(_pad_id, title_item) ) {
    while (!SystemEtc.IsNeedPausing()) {
      wait(0);
    }
    while ( SystemEtc.IsNeedPausing()) {
      wait(0);
    }
  }

  SystemEtc.updateTrialVersionStatus(); // 購入済み確認

  if ( !::util_is_trial_version() ) {
    ::g_systemdata.login_game_start(_pad_id, true);
  }
}

// 体験版か
function util_is_trial_version()
{
  // if ( ::isDebugBuild() )  
  //   return false; // XXX 常に完全版

  return SystemEtc.isTrialVersion();
}

// 指定したプレイヤーに割り当てられているコントローラがプライマリコントローラか
function util_is_play_at_primary_controller(_player_no)
{
  if ( SystemEtc.isExistPrimaryController() ) {
    local primaryID = SystemEtc.getPrimaryUserIndex();
    local paramLists_IDs = ::g_systemdata.get_value(SystemDataValueIndex.SETTING_PAD).get_IDs();
    return ( primaryID == paramLists_IDs[_player_no] );
  }
  else
    return true; // プライマリコントローラの概念がないシステムでは常にプライマリと判断
}

// ボタンを押してゲームを開始したPadのidを元に、各playerにidを自動割り当てする
function util_set_auto_pad_id(_key)
{
  local start_pad_id = ::util_get_start_pad_id(_key);
  ::util_set_auto_pad_id_by_pad_id(start_pad_id);
}
function util_set_auto_pad_id_by_pad_id(_start_pad_id)
{
  local paramLists_IDs = ::g_systemdata.get_value(SystemDataValueIndex.SETTING_PAD).get_IDs();
  local player1_pad_id_old = paramLists_IDs[0];
  paramLists_IDs[0] = _start_pad_id; // プレイヤー1に開始したpad_idを割当
  
  // プレイヤー2以降が存在すれば、1に割り当てたものと同じものがないように調整
  if ( !::util_is_enable_same_pad_id_assign() || (start_pad_id != player1_pad_id_old) ) {
    _util_set_auto_pad_id_fix_new_without_start_id(paramLists_IDs, _start_pad_id, player1_pad_id_old);
  }

  ::g_systemdata.get_value(SystemDataValueIndex.SETTING_PAD).set_IDs(paramLists_IDs);
  ::g_systemdata.get_value(SystemDataValueIndex.SETTING_PAD).complete_only_IDs(true);
}

// プレイヤー2以降が存在すれば、ゲームスタート用に割り当てたものと同じものがないように調整
function _util_set_auto_pad_id_fix_new_without_start_id(_paramLists_IDs, _start_pad_id, _old_pad_id)
{
  local playerNum = ::util_get_player_input_num();
  for (local player_no = 1 ; player_no < playerNum; player_no++) {
    local pad_id = _paramLists_IDs[player_no];
    if (_start_pad_id == pad_id) {
      _paramLists_IDs[player_no] = _old_pad_id; 
      break;
    }
  }
}

// アクティブで余っているidを検索
function _util_found_auto_pad_id_active_empty(_paramLists_IDs, _pad_id_player, _player_no_decide)
{
  local pad_id_active_empty = -1;
  local padConnected = ::g_inputHub.connected;
  local padNum = ::g_inputHub.inputNum;  // コントローラID数
  for (local pad_id = 0; pad_id < padNum; pad_id++) {
    if ( ( (1 << pad_id) & padConnected ) ) {
      // アクティブなコントローラ
      local is_empty_id = true;
      for (local player_no_check = 0; player_no_check < _player_no_decide; player_no_check++) {
        if (pad_id == _paramLists_IDs[player_no_check]) {
          is_empty_id = false;
          break;
        }
      }
      if (is_empty_id) {
        pad_id_active_empty = pad_id;
        printf("player_no:%d found empty active id %d.\n", _player_no_decide, pad_id);
        break;
      }
    }
  }
  return pad_id_active_empty;
}

// ダブってID設定されるのを回避
function util_set_pad_id_with_check_same(_paramLists_IDs, _decide_player, _old_id)
{
  if ( ::util_is_enable_same_pad_id_assign() ) {
    // ダブ割当許可タイトル
    return false;
  }
  else {
    local playerNum = ::util_get_player_real_num(); // ゲームの入力デバイス数ではなく、実際のプレイ人数分を確認
    local decideId  = _paramLists_IDs[_decide_player];
    printf("old_id = %s decide_id=%s\n", _old_id, decideId);
    for (local player_no = 0; player_no < playerNum; player_no++) {
      if ( player_no != _decide_player && decideId == _paramLists_IDs[player_no] ) {
        _paramLists_IDs[player_no] = _old_id;
      }
    }
  }

  return true;
}

// ポーズメニュー用のキーを取得
s_pad_key_for_menu <- KEY.SELECT;

function util_get_pad_key_for_menu()
{
  return s_pad_key_for_menu;
}

function util_update_pad_key_for_menu(_key)
{
  if (_key != null) {
    printf("util_update_pad_key_for_menu 0x%08x\n", _key);
    s_pad_key_for_menu = _key;
  }
}

//----------------------------------------------------------------------
// 汎用param操作用ユーティリティ

function get_layout(param, layoutKey="layout")
{
  local ret = null;
	
  switch (typeof param[layoutKey]) {
  case "array":
    ret = param[layoutKey];
    break;

  case "table":
    local bLang = false;
    foreach (key, val in param[layoutKey]) {
      if (key == s_default_language_tag) {
        bLang = true;
        break;
      }
    }
    if (bLang) 
      ret = param[layoutKey][ ::sys_get_language_tag() ];
    else 
      ret = param[layoutKey][ ::get_package_regionTag() ];
    break;
  }

  return ret;
}


function get_layout_value(param, index)
{
  return ::get_layout(param)[index];
}

function get_layout_index(param, itemKey)
{
  local ret = null;
	
  local layout = ::get_layout(param);
  foreach (i, val in layout) {
    if (val == itemKey) {
      ret = i;
      break;
    }
  }
	
  return ret;
}

function change_layout(param, itemKeyOld, itemKeyNew)
{
  local layout = ::get_layout(param);
  foreach (i, val in layout) {
    if (val == itemKeyOld) {
      layout[i] = itemKeyNew;
      break;
    }
  }
}

function remove_layout(param, itemKey)
{
  local layout = ::get_layout(param);
  foreach (i, val in layout) {
    if (val == itemKey) {
      layout.remove(i);
      break;
    }
  }
}

function insert_layout(param, itemKey, index)
{
  local layout = ::get_layout(param);
  layout.insert(index, itemKey);
}

function change_guide_key(param, guideKeyOld, guideKeyNew)
{
  local layout = ::get_layout(param);
  foreach (val in layout) {
    if (val) {
      local item = param[val];
      if (item && "guideKey" in item && item["guideKey"] == guideKeyOld) {
        item["guideKey"] = guideKeyNew;
      }
    }
  }
}

function swap_layout(param, itemKey1, itemKey2)
{
  local _index1 = null;
  local _index2 = null;
  local layout = ::get_layout(param);
  foreach (i, val in layout) {
    switch (val) {
    case itemKey1:	_index1 = i;	break;
    case itemKey2:	_index2 = i;	break;
    }
  }
  if (_index1 != null && _index2 != null) {
    layout[_index1] = itemKey2;
    layout[_index2] = itemKey1;
  }
}

function change_layout_state(param, itemKey, state)
{
  if ( itemKey in param ) {
    if ( "state" in param[itemKey] ) {
      param[itemKey]["state"] = state;
    }
    else {
      param[itemKey]["state"] <- state;
    }
  }
}
  
//------------------------------
// 
//------------------------------
function is_disable_on_trial_version(param, itemKey)
{
  if ( ::util_is_trial_version()
       && MENU_ITEMKEY_DISABLE_ON_TRIAL_VERSION in param[itemKey]
       && param[itemKey][MENU_ITEMKEY_DISABLE_ON_TRIAL_VERSION] ) {
    return true;
  }
  else {
    return false;
  }
}

//----------------------------------------------------------------------
// (デバッグ用) (呼び元threadの)コールスタックダンプ
//----------------------------------------------------------------------
function debug_dump_callstack()
{
  if (isMasterBuild()) return;

  local s = null;
  {
    local thread = ::getCurrentThread();
    local s = format("THREAD [%s] : status=", thread);
    local callstacks = [];
    local level = 0;
    local stack;
    while ((stack = thread.getstackinfos(level++)) != null) {
      callstacks.append(stack);
    }
    s += "\n";
    s += "CALLSTACK\n";
    foreach (i, callstack in callstacks) {
      s += format("*FUNCTION [%s()] %s line [%d]\n", callstack.func, callstack.src, callstack.line);
    }
    print(s + "\n");
  }
}


//言語設定に合わせたフォントファイルの取得
function getMultiFontPath( )
{
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

	local font_path = null;
/*
	switch( s_last_language )
	{
	case LANGUAGE_JPN:
		font_path = conv_path(MENU_MULTI_FONT_PATH);
		break;
	case LANGUAGE_ENG:
	case LANGUAGE_SPA:
	case LANGUAGE_FRA:
	case LANGUAGE_ITA:
	case LANGUAGE_GER:
		font_path = conv_path(MENU_US_MULTI_FONT_PATH);
		break;
	}
*/
	font_path = conv_path(MENU_MULTI_FONT_PATH);
	printf( "getMultiFontPath = %s\n", font_path );
	
	return( font_path );
}
//言語設定に合わせたフォントファイルの取得
function getTitleMultiFontPath( )
{
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

	local font_path = null;

/*
	switch( s_last_language )
	{
	case LANGUAGE_JPN:
		font_path = conv_path(MENU_MULTI_FONT32_PATH);
		break;
	case LANGUAGE_ENG:
	case LANGUAGE_SPA:
	case LANGUAGE_FRA:
	case LANGUAGE_ITA:
	case LANGUAGE_GER:
		font_path = conv_path(MENU_US_MULTI_FONT32_PATH);
		break;
	}
*/
	font_path = conv_path(MENU_MULTI_FONT32_PATH);
	printf( "getTitleMultiFontPath = %s\n", font_path );
	
	return( font_path );
}
//言語設定に合わせたフォントファイルの取得
function getMulti18FontPath( )
{
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

	local font_path = null;
/*
	switch( s_last_language )
	{
	case LANGUAGE_JPN:
		font_path = conv_path(MENU_MULTI_FONT18_PATH);
		break;
	case LANGUAGE_ENG:
	case LANGUAGE_SPA:
	case LANGUAGE_FRA:
	case LANGUAGE_ITA:
	case LANGUAGE_GER:
		font_path = conv_path(MENU_US_MULTI_FONT18_PATH);
		break;
	}
*/
	font_path = conv_path(MENU_MULTI_FONT18_PATH);
	printf( "getMultiFontPath = %s\n", font_path );
	
	return( font_path );
}

function getFont32Yoffset()
{
	local offsetY = 0;
/*
	switch( s_last_language )
	{
	case LANGUAGE_JPN:
		offsetY = 20;
		break;
	case LANGUAGE_ENG:
	case LANGUAGE_SPA:
	case LANGUAGE_FRA:
	case LANGUAGE_ITA:
	case LANGUAGE_GER:
		offsetY = 18;
		break;
	}
*/	
	offsetY = 18;
	return( offsetY );
}
function getFont24Yoffset()
{
	local offsetY = 0;
/*
	switch( s_last_language )
	{
	case LANGUAGE_JPN:
		offsetY = 14;
		break;
	case LANGUAGE_ENG:
	case LANGUAGE_SPA:
	case LANGUAGE_FRA:
	case LANGUAGE_ITA:
	case LANGUAGE_GER:
		offsetY = 12;
		break;
	}
*/	
	offsetY = 12;
	return( offsetY );
}
function getFont18Yoffset()
{
	local offsetY = 0;
/*
	switch( s_last_language )
	{
	case LANGUAGE_JPN:
		offsetY = 12;
		break;
	case LANGUAGE_ENG:
	case LANGUAGE_SPA:
	case LANGUAGE_FRA:
	case LANGUAGE_ITA:
	case LANGUAGE_GER:
		offsetY = 11
		break;
	}
*/	
	offsetY = 11
	return( offsetY );
}

//画面比率再設定
function setDisplayScale( width, height )
{
	if ( SCALETEST != 0 )	//スケール変更テストを行う場合は1
	{
		return;
	}

	//スクリーンサイズ設定
	local screenSetting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_05_last_screen();
	local num = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_05_last_screen();
	num = num + ( SCREENSETTING_MAX * ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_72_screenEffect( ) );	//セーブデータから取得する

	local scale_tbl = [ 
	                [ { x = 3.5, y = 3 }, 0, 0 ],  // 
	                [ { x = 3.75, y = 3.22 }, 0, 0 ],  // 
	                [ { x = 3, y = 3 }, 0, 0 ],  // 
	                [ { x = 5.01, y = 3.22 }, 0, 0 ],  // 
	                [ { x = 1.73, y = 1.33 }, 0, 1 ],  // 

	                [ { x = 3.5, y = 3 }, 1, 1 ],  // 
	                [ { x = 3.75, y = 3.22 }, 1, 1 ],  // 
	                [ { x = 3, y = 3 }, 1, 1 ],  // 
	                [ { x = 5.01, y = 3.22 }, 1, 1 ],  // 
	                [ { x = 1.73, y = 1.33 }, 0, 1 ],  // 
	              ];
/*
	local scale_tbl = [ 
	                [ { x = 3.5, y = 3 }, 0, 0 ],  // 
	                [ { x = 3.75, y = 3.21 }, 0, 0 ],  // 
	                [ { x = 3, y = 3 }, 0, 0 ],  // 
	                [ { x = 5, y = 3.21 }, 0, 0 ],  // 
	                [ { x = 1.73, y = 1.33 }, 0, 1 ],  // 

	                [ { x = 3.5, y = 3 }, 1, 1 ],  // 
	                [ { x = 3.75, y = 3.21 }, 1, 1 ],  // 
	                [ { x = 3, y = 3 }, 1, 1 ],  // 
	                [ { x = 5, y = 3.21 }, 1, 1 ],  // 
	                [ { x = 1.73, y = 1.33 }, 0, 1 ],  // 
	              ];
*/
	//画面位置
	local pos_tbl = [ 
	                [ { x = 0, y = 0 } ],  // 
	                [ { x = 0, y = 0 } ],  // 
	                [ { x = 0, y = 0 } ],  // 
	                [ { x = 0, y = 0 } ],  // 
	                [ { x = 0, y = 6 } ],  // 

	                [ { x = 0, y = 0 } ],  // 
	                [ { x = 0, y = 0 } ],  // 
	                [ { x = 0, y = 0 } ],  // 
	                [ { x = 0, y = 0 } ],  // 
	                [ { x = 0, y = 6 } ],  // 
	              ];

	local ys = 1;
	local xs = 1;
	local _rate = 1.0;
	local ofsx = 0.0;
	local ofsy = 0.0;

	local ratio = ::g_emu_task.getPixelAspectRatio();

	printf("ratio=%f\n", ratio );
	printf("setDisplayScale: width=%s, height=%s\n", ::EmuTask.getWidth(), ::EmuTask.getHeight());

  //GTスクリーンかを設定する
	local screenStting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_05_last_screen( );	//セーブデータから取得する
/*
	if( screenStting == SCREENSETTING_GT )
	{
		//GT
		xs = xs * ratio;	//
	}
	else
*/
	{
		if( width == 512 )
		{
			xs = xs * 0.5;	//512対応 
			printf("xs = xs * 0.5\n" );
			if( ::g_wallpaper )
			{
				::g_wallpaper.setScreen_sel();
			}
		}
		else if( width == 352 )
		{
			xs = xs * 0.7272;	//352対応 
			printf("xs = xs * 0.7272\n" );
			if( ::g_wallpaper )
			{
				::g_wallpaper.setScreen_sel();
			}
		}
		else if( width == 336 )
		{
			if ( screenSetting == 0 )	//縦3倍4：3
			{
				ofsx = 0.03;
				if( ::g_wallpaper )
				{
					::g_wallpaper.setScreen_sel(3);
				}
			}
			if ( screenSetting == 1 )	//上下ピッタリ4：3
			{
				ofsx = 0.04;
				ofsy = 0.02;
				if( ::g_wallpaper )
				{
					::g_wallpaper.setScreen_sel(4);
				}
			}
			if ( screenSetting == 2 )	//ピクセルパーフェクト
			{
				ofsx = 0.01;
				if( ::g_wallpaper )
				{
					::g_wallpaper.setScreen_sel(5);
				}
			}
			if ( screenSetting == 3 )	//フルスクリーン
			{
				ofsx = -0.02;
				ofsy = 0.02;
				if( ::g_wallpaper )
				{
					::g_wallpaper.setScreen_sel();
				}
			}
			xs = xs * 0.7634;	//336対応 
			printf("xs = xs * 0.7619\n" );
		}
		else if( width == 320 )
		{
			if ( screenSetting == 3 )	//フルスクリーン
			{
				ofsx = 0.00;
				ofsy = 0.01;
			}
			xs = xs * 0.8;	//320対応 
			printf("xs = xs * 0.8\n" );
			if( ::g_wallpaper )
			{
				::g_wallpaper.setScreen_sel();
			}
		}
		else
		{
			//256サイズの場合
			if ( screenSetting == 1 )	//上下ピッタリ4：3
			{
				ofsx = 0.00;
				ofsy = 0.00;
			}
			if ( screenSetting == 3 )	//フルスクリーン
			{
				ofsx = -0.02;
				ofsy = 0.02;
			}
			if( ::g_wallpaper )
			{
				::g_wallpaper.setScreen_sel();
			}
		}
	}
	
	
//	ofsx = 0.0;	//とりあえず無効にする
//	ofsy = 0.0;	//とりあえず無効にする

	local posx = pos_tbl[num][0].x;
	local posy = pos_tbl[num][0].y;

	//スーパーダライアスの見切れ対欧
	local rt = ::g_systemdata.get_value(SystemDataValueIndex.SETTING_ETC).get_game_regionTag( );
	if ( ( rt == "GAME010" ) || ( rt == "GAME010E" ) )
	{
			if ( screenSetting == 3 )	//フルスクリーン
			{
				//スーパーダライアス　かつ　フルスクリーン設定の場合は3ドットウィンドウ位置を下げる
				posy += 3;
			}
		
	}

	::g_emu_task.scanline  = scale_tbl[num][1] == 1 ? true : false;
	::g_emu_task.smoothing  = scale_tbl[num][2] == 1 ? true : false;

	::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).set_pos_x(posx) 
	::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).set_pos_y(posy) 
	::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).set_scanline(::g_emu_task.scanline);
	::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).set_smoothing(::g_emu_task.smoothing);
	::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).set_size_auto(ScreenSizeAutoIndex.MANUAL);  // 画面設定はマニュアル固定
	::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).set_manual_size_x(((scale_tbl[num][0].x * _rate * xs)+ofsx) * 100.0);
	::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).set_manual_size_y(((scale_tbl[num][0].y * _rate * ys)+ofsy) * 100.0);
	::g_systemdata.get_value(SystemDataValueIndex.SETTING_SCREEN).update_system(); // 反映
}


// koizumi add
const MAX_DIR = 1024;	//角度
// 円周率
const M_PI = 3.141592;
const PI_2 = 6.283184;

//ユーティリティ
function DirToDeg( dir )
{
	local rc = dir * PI_2 / MAX_DIR;
	
	return( rc );
}
//移動後の座標を取得
function DirMove( dir, spd, x, y )
{
	local move_x = 0;
	local move_y = 0;

	local rot = DirToDeg(dir);
	dir = dir % MAX_DIR;

	move_x = (  sin( rot ) * spd );
	move_y = ( -cos( rot ) * spd );

	
	local position = { x = x + move_x, y = y + move_y };
	return( position );
	
}

function getRund( max )
{
	local val = rand() % max;
	return val;
}

//目的地の角度を取得
function PointDir( sx, sy, ex, ey )
{
	local rc = 0;

	local px = ex - sx;
	local py = ey - sy;

	local dir = atan2(px, -py);
	local rot = dir * MAX_DIR / PI_2;
	rc = rot;
	
	return( rc );
}

function get_yure( count, haba, sokudo )
{
	local yure = ( sin(DirToDeg(count * sokudo)) * haba );
	return (yure.tointeger());
}



//絶対値を求める
function absf( val )
{
	if ( val < 0 )
	{
		val = -val;
	}
	return( val );
}

//近似値を求める
function Kinjiti( x1, y1, x2, y2 )
{
	local rc = 0;

	local absx = absf( x1 - x2 );
	local absy = absf( y1 - y2 );

	if ( absx > absy )
	{
		rc = ( absx + absy ) - ( absy / 2.0 );
	}
	else
	{
		rc = ( absx + absy ) - ( absx / 2.0 );
	}
	
	return( rc );
}

function getRund( max )
{
	local val = rand() % max;
	return val;
}

const OPTKINSPD_MAX = 9;
function TragetKinspeed_Option(start_x, start_y, end_x, end_y)
{
	
	local kinspdOptionX = [ 256, 128, 64, 32, 16, 8, 4, 2, 0];
	local kinspdOptionY = [ 128,  64, 32, 16,  8, 4, 2, 0, 0];
	
	local rc = 0;
	local sa = Kinjiti(start_x, start_y, end_x, end_y);
	local i;
	for (i = 0; i < OPTKINSPD_MAX; i++)
	{
		if (kinspdOptionX[i] < absf(sa))
		{
			rc = kinspdOptionY[i];
			if (sa < 0)
			{
				rc = -rc;
			}
			break;
		}
	}

	return (rc);
}

function getPkgRegion( )
{
		local devid = ::get_package_dev_id();
		local rc = null;
		rc = PKGREGION_JP;
/*
		switch( devid )
		{
		case DEVID_JP:
			break;
		case DEVID_US:
		case DEVID_EU:
		case DEVID_AS:
			rc = PKGREGION_US;
			break;
		}
*/
		return rc;
}
function getBGMotionName( )
{
	local rc = null;
	local indexFrameStting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_74_frameType( );	//セーブデータから取得する

	if ( s_last_linenp == LINEUP_US )
	{
		rc   = "bg_us";	//
	}
	else
	{
		switch( indexFrameStting )
		{
		case FRAME_PCENGINE:
			rc   = "bg";
			break;
		case FRAME_COREGRAFX:
			rc   = "bg_eu";
			break;
		}
	}
	return rc;
}
function getGameStartBGMotionName( )
{
	local rc = null;
	local indexFrameStting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_74_frameType( );	//セーブデータから取得する

	if ( s_last_linenp == LINEUP_US )
	{
		rc   = "bg_us";	//
	}
	else
	{
		switch( indexFrameStting )
		{
		case FRAME_PCENGINE:
			rc   = "bg";
			break;
		case FRAME_COREGRAFX:
			rc   = "bg_eu";
/*
			if ( ( s_last_language == LANGUAGE_JPN ) || ( s_last_language == LANGUAGE_ENG ) )
			{
				rc   = "bg_eu";
			}
			else
			{
				rc   = "bg";
			}
*/
/*
			local devid = ::get_package_dev_id();
			switch( devid )
			{
			case DEVID_JP:
				rc   = "bg_eu";
				break;
			case DEVID_US:
				rc   = "bg";
				break;
			}
*/
			break;
		}
	}
	return rc;
}

function getFrameTypeGuideText( inText )
{
	local rc = inText;
	
	if( inText )
	{
		local FrameStting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_74_frameType( );	//セーブデータから取得する
		if ( s_last_linenp == LINEUP_US )
		{
			if( inText == "Ι" )
			{
				rc = "И";
			}
			if( inText == "Κ" )
			{
				rc = "Л";
			}
			if( inText == "Μ" )
			{
				rc = "Ц";
			}
			if( inText == "Λ" )
			{
				rc = "П";
			}
		}
		else
		{
			switch( FrameStting )
			{
			case FRAME_PCENGINE:
				//変換処理なし
				break;
			case FRAME_COREGRAFX:
				if( inText == "Ι" )
				{
					rc = "Ж";
				}
				if( inText == "Κ" )
				{
					rc = "Б";
				}
				if( inText == "Μ" )
				{
					rc = "Г";
				}
				if( inText == "Λ" )
				{
					rc = "б";
				}
				break;
			}
		}
	}
	
	return(rc);
}


//ガイド表示クラス
class MenuModeGuide 
{
	m_icon = null;
	m_icon2 = null;
	m_text = null;
	
	m_gideX = null;
	m_gideY = null;
	m_iconX = null;
	m_iconY = null;
	m_icon2X = null;
	m_icon2Y = null;

	constructor( layer, posx, posy, icon, text, width = 180, colorChange = false ) 
	{

		if ( colorChange == true )
		{
			//テキストの色を変更する
			if ( s_last_linenp == LINEUP_US )
			{
			}
			else
			{
				local indexFrameStting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_74_frameType( );	//セーブデータから取得する
				switch( indexFrameStting )
				{
				case FRAME_PCENGINE:
					text = "#{color,00000FF}" + text;
					break;
				case FRAME_COREGRAFX:
					break;
				}
			}
		}

		local font_path = ::getMulti18FontPath( );

		local offsety_icon = 25;
		local offsety_text = getFont18Yoffset();
		local w_icon = 48;
		local icon2 = null;
		local offsety_icon2 = 25;
		if( icon == "Λ" )
		{
			w_icon = 100;
			offsety_icon = 17;
		}
		if( icon == "Μ" )
		{
			w_icon = 120;
			offsety_icon = 17;
		}
		
		
		if( icon == "＋" )
		{
			w_icon = 90 + 48;
			offsety_icon = 17;
			icon = "Λ"
			offsety_icon2 = 25;
			icon2 = "Ι"
		}


		//フレームタイプに合わせてアイコンの差し替え処理
		icon = getFrameTypeGuideText(icon);
		icon2 = getFrameTypeGuideText(icon2);

		m_icon = Indicator(layer, s_rsc.find(font_path));
		m_icon.visible = true;
		m_icon.setRecognizeTag(true);
		m_icon.fontColor = TEXT_COLOR_NORMAL;
//		m_icon.setAlignment(CONSOLE.ALIGNMENT_CENTER);
		m_icon.print(icon);
//		m_icon.setCoord( posx, posy - offsety_icon );
		IndicatorSetCoord( m_icon, posx, posy - offsety_icon )

		if( icon2 )
		{
			m_icon2 = Indicator(layer, s_rsc.find(font_path));
			m_icon2.visible = true;
			m_icon2.setRecognizeTag(true);
			m_icon2.fontColor = TEXT_COLOR_NORMAL;
	//		m_icon2.setAlignment(CONSOLE.ALIGNMENT_CENTER);
			m_icon2.print(icon2);
//			m_icon2.setCoord( posx + 100, posy - offsety_icon2 );
			IndicatorSetCoord( m_icon2, posx + 100, posy - offsety_icon2 )
		}

		m_text = Indicator(layer, s_rsc.find(font_path));
		m_text.visible = true;
		m_text.setRecognizeTag(true);
		m_text.fontColor = TEXT_COLOR_NORMAL;
//		m_text.setAlignment(CONSOLE.ALIGNMENT_CENTER);
		m_text.print(text);
//		m_text.setCoord( posx + w_icon, posy - offsety_text );
		IndicatorSetCoord( m_text, posx + w_icon, posy - offsety_text )

		local scale = 1;
		if( width )
		{
			local width1 = m_text.width;
			if( width < width1 )
			{
				scale = scale * width / width1;
				m_text.setFontScaleX(scale);
			}
		}
		
		local width2 = m_text.width;
		local cx = ( w_icon + 8 + width2 ) / 2;


		m_iconX = posx - cx;
		m_iconY = posy - offsety_icon;
//		m_icon.setCoord( m_iconX, m_iconY );
		IndicatorSetCoord( m_icon, m_iconX, m_iconY );

		if( m_icon2 )
		{
			m_icon2X = posx + 90 - cx;
			m_icon2Y = posy - offsety_icon2;
//			m_icon2.setCoord( m_icon2X, m_icon2Y );
			IndicatorSetCoord( m_icon2, m_icon2X, m_icon2Y );
		}
		m_gideX = posx + w_icon - cx;
		m_gideY = posy - offsety_text;
//		m_text.setCoord( m_gideX, m_gideY );
		IndicatorSetCoord( m_text, m_gideX, m_gideY );
		
  }

  //
  function setVisible( flg )
  {
		m_icon.visible = flg;
		m_text.visible = flg;
	}

  function setOffset( ofsy )
  {
//		m_icon.setCoord( m_iconX, m_iconY + ofsy );
		IndicatorSetCoord( m_icon, m_iconX, m_iconY + ofsy  );
		if( m_icon2 )
		{
//			m_icon2.setCoord( m_icon2X, m_icon2Y + ofsy );
			IndicatorSetCoord( m_icon2, m_icon2X, m_icon2Y + ofsy  );
		}
//		m_text.setCoord( m_gideX, m_gideY + ofsy );
		IndicatorSetCoord( m_text, m_gideX, m_gideY + ofsy  );
		
	}

}

//PCエンジン君クラス
const NECKUN_WAIT = 0;
const NECKUN_MOVE = 1;

const NECKUN_WAITTIME = 120;
class PceKun 
{
	m_motion = null;
	m_x = null;
	m_y = null;
	
	m_seq = null;
	m_wait = null;

	m_dir = null;
	m_spd = null;
	m_cnt = null;

	constructor( layer, visible = true ) 
	{
		local cx = SCREEN_XSIZE / 2;
		local cy = SCREEN_YSIZE / 2;

		m_x = getRund(SCREEN_XSIZE) - cx;
		m_y = getRund(SCREEN_YSIZE) - cy;

//		m_wait = getRund(NECKUN_WAITTIME * 2);
		m_wait = 0;
		m_dir = 0;
		m_spd = 0;
		m_seq = getRund(2);
		m_cnt = 0;

		m_motion         = Motion(layer);
		setWaitMotion(  );
		m_motion.opacity = 0;
		m_motion.visible = visible;
		m_motion.independentLayerInherit = true;
		m_motion.progress();
	}
	
	function exec( )
	{
		
		m_motion.opacity = m_motion.opacity + 8;
		if( m_motion.opacity > 255 )
		{
			m_motion.opacity = 255;
		}
		
		
//		printf( "neckun wait = %d\n", m_wait );
		m_wait--;
		if( m_wait < 0 )
		{
			m_wait = NECKUN_WAITTIME + getRund(NECKUN_WAITTIME);

			switch ( m_seq )
			{
			case NECKUN_WAIT:
				m_seq = NECKUN_MOVE;
				m_motion.chara   = "neckun";
				m_motion.motion  = "b";
				m_spd = 1 + getRund(2);
				m_dir = getRund(1024);
//				m_dir = m_cnt * 128;
				m_dir = m_dir % 1024;
				m_cnt++
				setWalkMotion(  );

				break;
			case NECKUN_MOVE:
				m_seq = NECKUN_WAIT;
				setWaitMotion(  );
				m_spd = 0;
				break;
			}
		}
		
		local pos = DirMove( m_dir, m_spd, m_x, m_y );
		
		m_x = pos.x;
		m_y = pos.y;

		m_motion.left = m_x;
		m_motion.top = m_y;

		local cx = SCREEN_XSIZE / 2;
		local cy = SCREEN_YSIZE / 2;
		if( m_x < -cx )
		{
			m_x = -cx;
			m_dir = getRund(512);
			setWalkMotion(  );
		}
		if( m_x > cx )
		{
			m_x = cx;
			m_dir = getRund(512) + 512;
			setWalkMotion(  );
		}
		if( m_y < -cy )
		{
			m_y = -cy;
			m_dir = getRund(512) + 256;
			setWalkMotion(  );
		}
		if( m_y > cy )
		{
			m_y = cy;
			m_dir = getRund(512) + 768;
			setWalkMotion(  );
		}

	}

	
	function setVisible( flg )
	{
		m_motion.visible = flg;
		if ( flg == false )
		{
			m_motion.opacity = 0;
		}
	}
	
	function setWaitMotion(  )
	{
		m_motion.setFlip(false, false);
		local type = getRund(5);
		m_motion.chara   = "neckun";
		switch ( type )
		{
		case 0:
			m_motion.motion  = "a";
			break;
		case 1:
			m_motion.motion  = "a2";
			break;
		case 2:
			m_motion.motion  = "a3";
			break;
		case 3:
			m_motion.motion  = "g";
			break;
		case 4:
			m_motion.motion  = "h";
			break;
		}
		m_motion.progress();
	}

	function setWalkMotion(  )
	{
		m_dir = m_dir % 1024;
		
		
		local type = 0;
		if ( ( m_dir > 960 ) && ( m_dir <= 64 ) )
		{
			type = 0;
		}
		if ( ( m_dir > ( 64 ) ) && ( m_dir <= 192  ) )
		{
			type = 1;
		}
		if ( ( m_dir > ( 192 ) ) && ( m_dir <= 320  ) )
		{
			type = 2;
		}
		if ( ( m_dir > ( 320 ) ) && ( m_dir <= 448  ) )
		{
			type = 3;
		}
		if ( ( m_dir > ( 448 ) ) && ( m_dir <= 576  ) )
		{
			type = 4;
		}
		if ( ( m_dir > ( 576 ) ) && ( m_dir <= 704  ) )
		{
			type = 5;
		}
		if ( ( m_dir > ( 704 ) ) && ( m_dir <= 832  ) )
		{
			type = 6;
		}
		if ( ( m_dir > ( 832 ) ) && ( m_dir <= 960  ) )
		{
			type = 7;
		}
		m_motion.chara   = "neckun";
		switch ( type )
		{
		case 0:
			m_motion.motion  = "f";
			m_motion.setFlip(false, false);
			break;
		case 1:
			m_motion.motion  = "e";
			m_motion.setFlip(true, false);
			break;
		case 2:
			m_motion.motion  = "d";
			m_motion.setFlip(true, false);
			break;
		case 3:
			m_motion.motion  = "c";
			m_motion.setFlip(true, false);
			break;
		case 4:
			m_motion.motion  = "b";
			m_motion.setFlip(false, false);
			break;
		case 5:
			m_motion.motion  = "c";
			m_motion.setFlip(false, false);
			break;
		case 6:
			m_motion.motion  = "d";
			m_motion.setFlip(false, false);
			break;
		case 7:
			m_motion.motion  = "e";
			m_motion.setFlip(false, false);
			break;
		}
		m_motion.progress();
	}

}

function setFrameinOut( )
{
	s_frame_offset = 30;
}
function FrameinOutExec( )
{
	s_frame_offset--;
	if( s_frame_offset < 0 )
	{
		s_frame_offset = 0;
	}
}
function getFrameOffset( )
{
	local ofs = ( s_frame_offset * s_frame_offset ) / 3;
	return( ofs );
}

function IndicatorSetCoord( indeicator, x, y )
{
	indeicator.setCoord( x.tointeger(), y.tointeger() );
}

//横幅に合わせてテキストを縮小する
function IndicatorSetXScale( indeicator, widthMAX )
{
		indeicator.setFontScaleX( 1 );
		local scale = 1;
		local width = indeicator.width;
		if( widthMAX < width )
		{
			scale = scale * widthMAX / width;
			indeicator.setFontScaleX(scale);
		}
}

function setLanguageTag( language )
{
	switch( language )
	{
	case LANGUAGE_ENG:
		::sys_set_language_tag(LANGUAGE_TAG_ENGLISH);
		break;
	case LANGUAGE_SPA:
		::sys_set_language_tag(LANGUAGE_TAG_SPANISH);
		break;
	case LANGUAGE_FRA:
		::sys_set_language_tag(LANGUAGE_TAG_FRENCH);
		break;
	case LANGUAGE_ITA:
		::sys_set_language_tag(LANGUAGE_TAG_ITALIAN);
		break;
	case LANGUAGE_GER:
		::sys_set_language_tag(LANGUAGE_TAG_GERMAN);
		break;
	case LANGUAGE_JPN:
		::sys_set_language_tag(LANGUAGE_TAG_JAPANESE);
		break;
	}
}

//設定言語を取得する
function getlastLanguage()
{
	return s_last_language;
}

