// m2-publish: turn a PCE Mini game library on disk into the deployable
// m2engage layout (roms/, folders/<lineup>/<folder>/<files>).
//
// Library layout (per-game; single-level folders only):
//
//   library/<lineup>/                       ← jp or us
//     gamelist.json                         ← [{folder, sor_*}] — games AND folders, in display order
//     <gameDirName>/                        ← regular game (folder name does NOT start with "FOLDER")
//       game.json                           ← display, emulator, rom, cover, cover_size
//       <rom_filename>                      ← e.g. KungFu_J.pce
//       cover.png
//       sram.bin                            ← 8448 B; one slot of _92_sram_datas
//       saves/
//         state_0.bin .. state_3.bin        ← slot-agnostic; publisher renames at publish time
//         state_0_meta.bin .. state_3_meta.bin
//     FOLDER<id>/                           ← folder entry (folder name starts with "FOLDER")
//       folder.json                         ← {name: "Display name"}
//       gamelist.json                       ← folder's own [{folder, sor_*}]
//       cover.png                           ← folder's icon (shown as a card in parent menu)
//       <gameDirName>/...                   ← games inside the folder (same per-game shape)
//
// Published layout:
//
//   published/
//     game/                                 ← bind-mounted over /usr/game/ at boot
//       system/                             ← scripts, system PSBs, fonts, motions, sounds
//         roms/
//           <rom>.pce.m  (HuCard, MZS-packed)
//           <rom>.pcd    (CD, raw)
//       040/                                ← active-pack overlay target
//       lib/m2hook_print.so
//       save/
//         data_008_0000.bin                 ← BACKUP_FLAGS + active-pack SRAM (engine read/write)
//         data_011_*.bin / data_012_*.bin   ← active-pack state files
//     folders/<lineup>/                     ← bind-mounted over /UDISK/folders/
//       _root/                              ← depth-0 menu (real games + folder cards)
//         title_prof.psb.m
//         title_mode_top.psb.m
//         title_jp_titleselect_<lineup>.psb.m
//         saves/
//           data_011_*.bin / data_012_*.bin + meta (engine-format, slot = game_index*4+N+lineup_offset)
//           sram.bin                        ← 150 × 8448-byte blocks (per-pack concatenation)
//       <FOLDER<id>>/                       ← one pack per subfolder, same shape

mod library;
mod publish_pipeline;
mod sync;
mod templates;
mod title_mode_top;
mod title_prof;
mod title_select;

pub use library::{CoverSize, Folder, Game, GameRom, Library, Lineup, LineupEntry};
pub use publish_pipeline::{publish, PublishOptions, PublishReport};
pub use sync::{sync_library_from_published, SyncOptions, SyncReport};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("mzs: {0}")]
    Mzs(#[from] m2_mzs::Error),
    #[error("psb: {0}")]
    Psb(#[from] m2_psb::Error),
    #[error("template: {0}")]
    Template(String),
    #[error("library: {0}")]
    Library(String),
}
