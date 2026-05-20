// デバッグループテストスイッチ
// 連続してデモのタイトルを切り替え続けるテストをする場合は1にする
const MODE_DEMO_LOOP_TEST = 0;
// デバッグループテスト時の待ち時間
const MODE_DEMO_START_COUNT_DEBUG = 600;
const MODE_DEMO_PLAY_COUNT_DEBUG  = 600;

// メニューで放置してデモが始まるまでの時間
const MODE_DEMO_START_COUNT = 3600; // 1分
//const MODE_DEMO_START_COUNT = 1800; // テストで30秒にする
// デモのプレイ時間
const MODE_DEMO_PLAY_COUNT  = 1800; // ３０秒

// 強制リブートさせるまでの全体ループ回数
const MODE_DEMO_REBOOT_COUNT = 2;

const INDEXMAX = 100;	//1言語のテーブルの最大値は100

class ModeDemo
{
  m_demo_start_count = 0;
  m_demo_play_count  = 0;

  m_titles = null;
  m_config = null;

  m_count_all = 0;
  m_count = 0;
  m_index = 0;

  m_layer = null;
	m_text_Debug = null;

  constructor()
  {
    m_count_all = 0;
    m_count = 0;
    m_index = 0;

    local current_dev_id = ::get_current_title_dev_id();
    local title_list = ::get_package_title_dev_name_list();
    m_titles = [];
    for (local no = 0; no < title_list.len(); no++) {
      local title = ::get_title_item(title_list[no]);
      m_titles.append( ::get_item_string(title["dev_name"]) );
    }
    m_config = ::util_load_config( ::conv_path("config/title_mode_top.psb") );

    if( MODE_DEMO_LOOP_TEST )
    {
      m_demo_start_count = MODE_DEMO_START_COUNT_DEBUG;
    }
    else
    {
      m_demo_start_count = MODE_DEMO_START_COUNT;
    }
    
		if ( MODE_DEMO_DEBUGDISP == 1 )
		{
	    m_layer = ScaledLayer();
	    m_layer.visible = true;
	    m_layer.smoothing = MOTSMOOTHING;
			m_layer.priority = 9999;

			local font_path = ::getMulti18FontPath( );
			local rsc = Resource();
			rsc.load( font_path );
			while (rsc.loading)
			  wait(0);

			m_text_Debug = Indicator(m_layer, rsc.find(font_path));
			m_text_Debug.visible = true;
			m_text_Debug.setRecognizeTag(true);
			m_text_Debug.fontColor = TEXT_COLOR_NORMAL;
			m_text_Debug.setAlignment(CONSOLE.ALIGNMENT_CENTER);
			IndicatorSetCoord( m_text_Debug, 0, 340 );

		}

  }

  // どこかでカウントをリセットしたいときに呼び出す
  function init(_mode=-1)
  {
    switch( _mode )
    {
    case 0:
      m_count = 0;
      m_index = 0;
    break;
    // 通常時はデモ開始までのカウントだけ初期化
    default:
      m_count = 0;
    break;
    }
  }

  // FOLDER HACK: re-read title_mode_top from disk so the demo picks games
  // from the currently-swapped lineup pack, not the snapshot taken at boot.
  // Called from mode_title_select.nut on lineup toggle. The script-level
  // s_loaded_config slot must already have been dropped by the caller.
  function reload_config()
  {
    m_config = ::util_load_config( ::conv_path("config/title_mode_top.psb") );
    m_count = 0;
    // Reset to start of current lineup's range (next() handles bounds too).
    m_index = this._lineup_range()[0];
  }

  // _parent MenuModeTitleSelectSub
  function check(_parent)
  {
    // No demo mode inside folders — m_config holds parent lineup data
    // which doesn't match the folder's title_prof.
    // s_in_folder is set/cleared by the csize 10/11 handler in
    // mode_title_select.nut. (Originally this called ::isInGameFolder() —
    // a m2engage-mac C++ binding that doesn't exist in stock m2engage.)
    if (::s_in_folder) return false;

    if ( !::g_input.key(KEY.ALL & ~KEY.R5) )	// 3Button 判定用のキーだけマスク
    {
      m_count++;
      if( m_count > m_demo_start_count )
      {
        return true;
      }
    }
    else
    {
      m_count = 0;
    }

    return false;
  }

  // FOLDER HACK: lineup-aware demo index range.
  // title_mode_top items: slots 0..49 = JP games, 50..99 = US games.
  function _lineup_range()
  {
    if (s_last_linenp == LINEUP_JP) return [0, LINEUPMAX];        // 0..49
    else                            return [LINEUPMAX, LINEUPMAX*2]; // 50..99
  }

  function next()
  {
    local r = this._lineup_range();
    local lo = r[0];
    local hi = r[1];
    if (m_index < lo || m_index >= hi) m_index = lo;
    m_index++;
    if( m_index >= hi )
    {
      m_index = lo;
      m_count_all++;
    }
    printf("m_index %d (lineup range %d..%d) m_count_all %d\n", m_index, lo, hi, m_count_all );
  }

  function exec(_parent)
  {
    printf("demo exec\n");

    local game_exit = 1;

    m_count = 0;
    // FOLDER HACK: ensure m_index is within current lineup's range
    // (lineup may have changed since the last demo cycle).
    {
      local r = this._lineup_range();
      if (m_index < r[0] || m_index >= r[1]) m_index = r[0];
    }
    while( game_exit == 1 )
    {
/*
      local index       = getlastIndex( m_index );
      local indexoffset = getLanguageIndexOffset( );
      local lists_index = index + indexoffset;
*/
      local lists_index = m_index;	//PCEは言語でロムが変わらないので、そのままインデックスで流す

      printf( "lists_index = %d\n", lists_index );

      local result = null;
      local item = m_config["items"][lists_index];
      for (local no = 0; no < m_titles.len(); no++)
      {
        if ( item["dev_name"] == m_titles[no] )
        {
          result = no;
          break;
        }
      }


      if( result != null )
      {
//        _parent.after_change_title();
        local regionTag =  m_config["items"][lists_index]["regionTag"];
printf( "-------\n" );
printf( "%d %s %s\n", m_index, regionTag, m_config["items"][lists_index]["name"] );
printf( "-------\n" );
        // ダミーデータはカウントアップして次に行く
        if( regionTag == "DUMMY" )
        {
          this.next();
          continue;
        }
        // Skip folder entries — they are not playable games
        if( regionTag.find("FOLDER_") == 0 || regionTag == "FOLDER_BACK" )
        {
          this.next();
          continue;
        }
        ::mode_main_change_title(result);//タイトルの切り替えはダミーの判定後にする

        ::g_systemdata.get_value(SystemDataValueIndex.SETTING_ETC).set_game_regionTag(regionTag);

        if( MODE_DEMO_LOOP_TEST )
        {
          m_demo_play_count = MODE_DEMO_PLAY_COUNT_DEBUG;
        }
        else
        {
	        // title_mode_top.jsonに"demo_time"タグがあればデモ表示時間を変更
/*
		      local indexoffset = getLinenpIndexOffset( );
		      local demoTimeindex = index + indexoffset;
*/
	        if( "demo_time" in m_config["items"][lists_index] )
	        {
	          m_demo_play_count = m_config["items"][lists_index]["demo_time"];
	        }
	        else
	        {
	          m_demo_play_count = MODE_DEMO_PLAY_COUNT;
	        }
printf( "-------\n" );
printf( "%d m_demo_play_count = %d %s\n", lists_index, m_demo_play_count, m_config["items"][lists_index]["name"] );
printf( "-------\n" );
	      }
				local screenMode =  m_config["items"][lists_index]["ScreenMode"];
				::g_emu_task.SetScreenMode( screenMode );	//個別に画面サイズ対応
				local screenOfsX =  m_config["items"][lists_index]["EmuOfsX"];
				local screenOfsY =  m_config["items"][lists_index]["EmuOfsY"];
				::g_emu_task.SetEmuScreenOffsetX( screenOfsX );	//個別に画面サイズ対応
				::g_emu_task.SetEmuScreenOffsetY( screenOfsY );	//個別に画面サイズ対応

        ::g_menu_sound.pause_bgm(true);
        {
          game_exit = mode_demo();
          this.next();
        }
      }
      else
      {
        break;
      }
    }

    return 0;
  }


  function mode_demo()
  {
    local game_exit = 0;

    ::request_re_init_emulator(); // emulator 再初期化 リクエスト登録（引数無し：systemセーブデータを使用）

    // デモモード開始
    ::util_load_script( conv_path(SCRIPT_PLAY_STANDALONE_PATH) );
    {
      local bgm = MenuSoundLockBgmRegion(::g_menu_sound);
      game_exit = play_standalone_demo();
    }

    hookEmulateOriginalBug(); // Original Bug Emulate フック解除

    return game_exit;
  }


  function play_standalone_demo()
  {
    local game_exit = 0;

    ::g_emu_task.pause = true;
    ::g_emu_task.visible = false;

//    ::g_menu_sound.pause(true);

//    ::g_frameCount.setGamePlay(1);	//シャットダウン用のステータスを設定
    {
      ::g_emu_task.pause   = false;
      ::g_emu_task.visible = true;

      ::play_standalone_start_emulator();

      ::g_systemdata.update_system_all();

//      ::util_load_script( conv_path("system/script/play_standalone_sub.nut") ); // play_standalone / pause_main のサブ処理

      local title_control = _play_standalone_create_title_control(::g_emu_task, false);
//      ::play_standalone_load_check_when_start(title_control); // NormalMode開始時のstatedataチェック&ロード
      ::play_standalone_try_reset_emulator(false, title_control);
      {
        game_exit = _play_standalone_gameloop_demo(title_control);
      }
      _play_standalone_delete_title_control(::g_emu_task, false, title_control);

      ::g_emu_task.pause   = true;
      ::g_emu_task.visible = false;

      _play_standalone_end_emulator(); // エミュレータ終了
    }
//    ::g_frameCount.setGamePlay(0);	//シャットダウン用のステータスを設定

//    ::g_menu_sound.pause(false);

    return game_exit;
  }

  // out -1:電源ボタン 1:時間終了 2:キー終了
  function _play_standalone_gameloop_demo(_title_control)
  {
    local game_exit   = 0;
    local demo_count  = 0;


		local screenStting = ::g_systemdata.get_value(SystemDataValueIndex.BACKUP_FLAGS).get_05_last_screen( );	//セーブデータから取得する
		local GTDisplayAlpha = 0;
		if( screenStting == SCREENSETTING_GT )
		{
			//GT
			::g_wipe.close(false, 0);
		}
		::g_menu_sound.fade_insert_cd();	//CDのシーク音を止める

    while ( !game_exit )
    {
      // 押されているボタンがゲームに反映されないようにする
      for ( local i = 0 ; i < g_inputHub.getInputNum() ; i++ )
      {
        ::g_emu_task.setSystemKeyFilter( i, g_inputHub.inputAt(i).key(KEY.ALL) );
      }

      if ( _title_control )
      {
        _title_control.sys_mainloop(false, ::g_emu_task, ::g_systemdata); // タイトルごとに毎フレーム実行する処理(ステートとして残らないことに注意)
			}

			//GT壁紙対応
			if( screenStting == SCREENSETTING_GT )
			{
				GTDisplayAlpha++;
				if( GTDisplayAlpha > 255 )
				{
					GTDisplayAlpha = 255;
					screenStting = 0;	//画面設定をやめる
				}
				::g_emu_task.setBrightness( GTDisplayAlpha / 255.0 );
				
//				printf( "GTDisplayAlpha = %d\n", GTDisplayAlpha );
			}
			
			if( MODE_DEMO_DEBUGDISP == 1 )
			{
		    local str = "#{color,808080FF}demo_count" + demo_count + " / m_demo_play_count " + m_demo_play_count;
				m_text_Debug.print(str);
			}

      //パワーボタン検出
      local fseq = ::g_frameCount.getFinishSeq();
      if ( fseq > 0 ) 
      {
        printf("finishifunc 1\n");
        //保存する
        ::g_systemdata.TryAutosave(true, true);
        game_exit = -1;
        ::g_frameCount.setFinishSeq(2);  //シャットダウン要求

        printf("finishifunc 2\n");
        ::g_frameCount.appFinish();  //シャットダウン
        break; // モード終了
      }
      else if( ::g_input.key(KEY.ALL & ~KEY.R5) )
      {
      	// キー入力で終了
        game_exit = 2;
        break;
      }
      else
      {
	      // 時間で終了
	      if( ++demo_count > m_demo_play_count )
	      {
	        game_exit = 1;
	        break;
	      }
	    }
      wait(0);
    }
    return game_exit;
  }
}

::g_demo_control = ModeDemo();

