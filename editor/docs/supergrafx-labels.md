# SuperGrafx HuCard label lookup

The title-selection sheet has two separate image-indexed motions:

- `object.pkg.motion.front`: the game covers in the carousel;
- `object.pkg.motion.sg`: the label on the HuCard during SuperGrafx launch.

`mode_title_select.nut::gameStart()` chooses `boot_sg` for `csize == 1` and
passes the selected item's `image` to the animation's `pkg` variable. In the
original `system/motion/titleselect_ui.psb.m`, the `hucard_sg` motion references
`src="pkg", icon="sg"` in the JP, EU and US background objects.

## Comparison of the actual archives

The following values were decoded directly from the stock JP cover archive
and the generated `folders/jp/FOLDER_SGX` pack, rather than inferred from game
directory names:

| Game | Stock `sg` frame time | Stock image reference | Folder `items[].image` | Correct generated reference |
|---|---:|---|---:|---|
| Daimakaimura | 7 | `tex#001 / 0045` | 1 | `tex#003 / 0001` |
| Aldynes | 25 | `tex#000 / 0000` | 2 | `tex#004 / 0001` |

Folder image 0 is the back card. The game directory `GAME030` does **not** mean
that Aldynes uses frame 30: the stock sheet places its image at frame 25.

Previously, the publisher rebuilt `front` for indices 0, 1 and 2 but copied
`sg` unchanged from stock. Its old frames at 7 and 25 therefore did not select
the new covers. At indices 1 and 2, the preceding stock frame at time 0 was
still selected; it points to `src="front", icon="大魔界村_修正後"`, which does not
resolve to a texture source in the sheet. This explains the blank launch label.

## Generator correction

[`title_select.rs`](../crates/m2-publish/src/title_select.rs) now rebuilds the
lookup frames and timing of both `front` and `sg` from the same ordered covers.
Both motions reference the **same texture streams**. No second cover image or
additional texture allocation is introduced for the launch animation.

Stock texture atlases, `thumb`, `soft31..33` and the other carousel layers remain
unchanged. For the US template, which has no stock `sg` motion, a single-layer
track is derived from its primary `front` layer. This supplies the `pkg/sg`
reference if a custom US lineup contains SuperGrafx games.

The rebuilt real SGX archive was decoded and compared against the old one:
only the `sg` motion changed, all extracted texture images were byte-identical,
and indices 1/2 resolved to the same covers as `front`. Its final frame and
parameter range now match `front` (sentinel 52, `rangeEnd`/`division` 51).

Three asset-free regression tests cover folder indices, moved games through
slot 49, stock texture preservation, texture sharing, MZS/PSB round trips and a
template without `sg`. Run them with:

```sh
cd editor
cargo test -p m2-publish
```

Existing published packs need to be regenerated; changing the publisher alone
does not modify archives already deployed to a console or VM. Restart M2 after
replacing a sheet so it loads the corrected motion instead of its cached copy.
