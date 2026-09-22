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
// Canonical USB layout:
//   game/                         original native engine + extracted resources
//     system/script/*.nut.m       bundled Chronos scripts, refreshed on publish
//     system/roms/                mount point for library/published/roms
//     lib/m2hook_print.so          bundled hook
//     save/                       existing live saves (never modified here)
//   library/
//     jp/, us/, templates/        editable library and stock templates
//     published/
//       roms/                     shared published ROM set (no copies in game/)
//       save/data_008_0000.bin     published settings/SRAM
//       folders/<lineup>/
//         _root/ or <folder>/
//           title_prof.psb.m
//           title_mode_top.psb.m
//           title_jp_titleselect_<lineup>.psb.m
//           saves/                engine-format states and per-pack sram.bin
//
// Other output paths remain standalone exports (roms/, folders/, save/).

mod library;
mod publish_pipeline;
mod sync;
mod templates;
mod atlas;
pub mod fonts;
pub mod title_preview;
pub fn decode_resource(bytes: &[u8], name: &str) -> Result<Vec<u8>, Error> {
    Ok(m2_mzs::unpack_default(bytes, name)?)
}
mod title_mode_top;
mod title_prof;
mod title_select;
mod usb;

pub use library::{CoverSize, Folder, Game, GameRom, Library, Lineup, LineupEntry};
pub use publish_pipeline::{publish, PublishOptions, PublishReport};
pub use sync::{sync_library_from_published, SyncOptions, SyncReport};
pub use usb::UsbPreparation;

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
