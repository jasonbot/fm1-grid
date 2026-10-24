# Embedded firmware

The images compiled into the binary are listed in `manifest.txt` (`file | name | version |
sha256 | tags`); `build.rs` turns it into the table used by `src/builtin.rs`. Manage the list
with `scripts/firmware.sh add/del/list` and rebuild. The tags are The FM-1 Firmware Hub's
categories (`kind`) plus its vintage/beta flags; the first tag is the group the firmware
dropdowns show it under. The table below is the human-maintained record of where each package
came from; the SHA-256 hashes were checked against the hub's catalogue on 2026-10-23. The RedtailWorks
pack (HYP-1, LOF-1, ECO-1, GRAV-1, CTY-1) is not hub-listed except HYP-1, whose hash matches the
catalogue; the other four packages come from RedtailWorks' own installer pages, hashes recorded on
download on 2026-10-23.

| Choice | Version | File | Tags | Source | SHA-256 | Licence |
|--------|---------|------|------|--------|---------|---------|
| SLOOP | 2.8.1 | `sloop-2.8.1.fwsc` | groove | <https://thefirmwarehub.com/fw/sloop-2.8.1.fwsc> · <https://github.com/isod89/sloop-fm1> | `64c9dfa7e75312debeae01a7c96e4ad3801eaffff9c81cb76ece9c48daf0501d` | GPL-3.0 |
| Felucca | 1.6.1 | `felucca-1.6.1.fwsc` | groove,dev | <https://thefirmwarehub.com/fw/felucca-1.6.1.fwsc> · <https://github.com/hugelton/Felucca> | `dff3a38f7e5b9833e15aba322558c9485e9e422e3ff813c616c91b3584a2d909` | GPL-3.0 |
| FM1 Open | V15 | `FM-1_015.fwsc` | official | <https://thefirmwarehub.com/fw/mvave/FM-1_015.fwsc> · <https://github.com/m-vave/fm1_open> | `db1642b2b6fa5c2cccb11ffd13878068bb28601678d3644049f99dc40e7edb8a` | Apache-2.0 |
| ChoralRoot | 0.26.1 | `choralroot-0.26.1.fwsc` | chords,beta | <https://thefirmwarehub.com/fw/choralroot-0.26.1.fwsc> · <https://github.com/Quixotic7/ChoralRootFM1> | `7fec6a729c2d05d1f19e8b6ab3e2101affb8d9b11e4835cb412b11625289ace2` | GPL-3.0 |
| Chipbox | 1.2.4 | `chipbox-1.2.4.fwsc` | groove | <https://thefirmwarehub.com/fw/chipbox-1.2.4.fwsc> · <https://github.com/pbarilla/chipbox> | `0a6df3248c07b17587badcc6258d8bce184252994c2dc6309eb09db90f5e6a42` | GPL-3.0 |
| DUB-1 | 1.3.0 | `dub-1.3.0.fwsc` | groove | <https://thefirmwarehub.com/fw/dub-1.3.0.fwsc> · <https://github.com/sbegueria/fm1-dub> | `35a464983c8b0f1143e668db770f36d943df0279a477be5e61435325d9354796` | GPL-3.0 |
| GHOULBOX | 2.3.2 | `ghoulbox-2.3.2.fwsc` | groove | <https://thefirmwarehub.com/fw/ghoulbox-2.3.2.fwsc> · <https://github.com/jasonpersinger/ghoulbox-fm1-dungeon-synth> | `069f6d783db1e05776a89006ecba07a0abfcd2cc7a0fe6fa64db096b5ecad4c8` | GPL-3.0 |
| GR!MM | 1.0 | `grimm-1.0.fwsc` | groove | <https://thefirmwarehub.com/fw/grimm-1.0.fwsc> · <https://github.com/GRiMMBIENT/grimm> | `8f1f6c0bc6c0afb58a400f4ea99a7a03302b98b38e4905527aa1fa2c665f5f65` | GPL-3.0 |
| Jangada | 1.0.1 | `jangada-1.0.1.fwsc` | groove | <https://thefirmwarehub.com/fw/jangada-1.0.1.fwsc> · <https://github.com/zednaked/jangada> | `c382e334057bd107294940e456e5c1140754917ba152a1958af3aa1696333588` | GPL-3.0 |
| JIANT FM | 0.9.9-beta4a-813869d | `jiant-0.9.9-beta4a-813869d.fwsc` | groove,beta | <https://thefirmwarehub.com/fw/jiant-0.9.9-beta4a-813869d.fwsc> · <https://github.com/juanjiant-bit/JIANT> | `3d73c59a591dd73562e3314d5081cdb0ccc9bcb187e85434f5cb5a27f627e9e1` | GPL-3.0 |
| LOOM-8 | 0.6.1 | `loom-0.6.1.fwsc` | groove,beta | <https://thefirmwarehub.com/fw/loom-0.6.1.fwsc> · <https://github.com/charlesvestal/fm1-loom> | `0849c89312d7028d4c735eccd293245939ddb2115b1a699453751cc5d59084bb` | GPL-3.0 |
| MAKU | 1.0 | `maku-1.0.fwsc` | groove | <https://thefirmwarehub.com/fw/maku-1.0.fwsc> · <https://github.com/okumurari2/maku> | `b55d471fd179008511dfea032530750f0692d5be8eab263970f5c3c2162b53ea` | GPL-3.0 |
| Melodee | 1.4.5 | `melodee-1.4.5.fwsc` | groove | <https://thefirmwarehub.com/fw/melodee-1.4.5.fwsc> · <https://github.com/keremimo/melodee> | `a18f40d6f8c8087b25f115ab11286f85c7f04383de836227c463c6ce4745ce20` | GPL-3.0 |
| Mutable Dreams | 1.0 | `mutable-1.0.fwsc` | groove | <https://thefirmwarehub.com/fw/mutable-1.0.fwsc> · <https://github.com/kidbee85-beep/mutable-dreams> | `75008881683cbeb3515e24cfd7830136b90eb293f0752dc66b203b7f2ee7cd9f` | GPL-3.0 |
| Polly | 0.5 | `polly-0.5.fwsc` | groove,vintage,beta | <https://thefirmwarehub.com/fw/polly-0.5.fwsc> · <https://github.com/charlesvestal/fm1-polly> | `2dea2914b4628f075cf3605fae7a75afc03a4b2ae2d7823d8ccf4bf16dd986c7` | GPL-3.0 |
| Skiff | 1.4 | `skiff-1.4.fwsc` | groove | <https://thefirmwarehub.com/fw/skiff-1.4.fwsc> · <https://github.com/illuminus5/skiff> | `0622a752e8304777c042ccea0732ba023e1413baa6dbc8724576e2ec76de02e0` | GPL-3.0 |
| TRIBE | 0.3-beta | `tribe-0.3-beta.fwsc` | groove,beta | <https://thefirmwarehub.com/fw/tribe-0.3-beta.fwsc> · <https://github.com/apiel/fm1-tribe> | `daa953ea5c37a5612d6db6f508a86879a79214768acc79d6ae04f2e0b93b6f6b` | GPL-3.0 |
| X0X | 1.1.0 | `x0x-1.1.0.fwsc` | groove,vintage | <https://thefirmwarehub.com/fw/x0x-1.1.0.fwsc> · <https://github.com/charlesvestal/fm1-x0x> | `a0c29deb930c65f1d40b5691b7d673f861418b438b7e65027cb9c671e6f44f5b` | GPL-3.0 |
| CoralFlower | 0.5 | `coralflower-0.5.fwsc` | chords,beta | <https://thefirmwarehub.com/fw/coralflower-0.5.fwsc> · <https://github.com/FicusFicusov/CoralFlower> | `ab7d0744eed0c83d62be39da9d63c5618e1bccc94de2931a9145a7d4b723b0bb` | GPL-3.0 |
| FoMni | 0.4 | `fomni-0.4.fwsc` | chords,vintage,beta | <https://thefirmwarehub.com/fw/fomni-0.4.fwsc> · <https://github.com/charlesvestal/fm1-fomni> | `71fa8304b91bff89078bfd27e981ed28ca4f0d3c5f406e8fdb5a2ab240fbdbb5` | GPL-3.0 |
| FuMi-1 | 1.1.0 | `fumi-1.1.0.fwsc` | chords,vintage | <https://thefirmwarehub.com/fw/fumi-1.1.0.fwsc> · <https://github.com/cartesive/fumi-1> | `211db1c2efc379b0a29a284aea523c67e230d98dab7a7f91336cc3fbd1201188` | GPL-3.0 |
| AMB-1 | 1.3 | `amb-1.3.fwsc` | synth | <https://thefirmwarehub.com/fw/amb-1.3.fwsc> · <https://github.com/charlesvestal/fm1-amb> | `59dbc6b688e3a64bb0da912c1e40890da7bc214b2a3269704627369b776707cb` | GPL-3.0 |
| Clipper | 1.1-experimental | `clipper-1.1-experimental.fwsc` | synth,beta | <https://thefirmwarehub.com/fw/clipper-1.1-experimental.fwsc> · <https://github.com/isod89/clipper-fm1> | `3a23a37c431c813bcdf97321f49722c7328d1f9134ba469ff3ba194ad9f05206` | GPL-3.0 |
| FiMba-1 | 0.6.1 | `fimba-0.6.1.fwsc` | synth,beta | <https://thefirmwarehub.com/fw/fimba-0.6.1.fwsc> · <https://github.com/jadamsowers/fm1-fimba> | `2ce744411ab6142a27e5c6238b8eb2f3875ccaa1e6fc47726211bf09f1393fa7` | GPL-3.0 |
| Cut-A-Dor | 1.2.1 | `cutador-1.2.1.fwsc` | drums | <https://thefirmwarehub.com/fw/cutador-1.2.1.fwsc> · <https://github.com/kidbee85-beep/cut-a-dor-release> | `5d4be92061cc5bf1396d3ff599ac3a1ef9ec1261afe003b74e32f4298307f614` | GPL-3.0 |
| 386DX | 1.0.2 | `386dx-1.0.2.fwsc` | other | <https://thefirmwarehub.com/fw/386dx-1.0.2.fwsc> · <https://github.com/seajaysec/386-DX-FM1-KaraokeBot> | `97dc787a7b5ef4052c6ab519e98a679274e1fd1c25ba46baa3a3d02901008028` | GPL-3.0 |
| Fart Machine One | 1.1.0 | `fart-1.1.0.fwsc` | other | <https://thefirmwarehub.com/fw/fart-1.1.0.fwsc> · <https://github.com/adamcharleswilsonmusic-code/Fart-Machine-One> | `b9e380061ddc086d5c45cfa4d4e583249a9bfb45869e1fcd6fdaf5249c7ee7e2` | GPL-3.0 |
| MIN-1 | 0.2.1 | `min1-0.2.1.fwsc` | other,beta | <https://thefirmwarehub.com/fw/min1-0.2.1.fwsc> · <https://github.com/Reytian/fm1-min1> | `82715d6ee18c8b12c287d614c131a4fb37f8606d811ae23301f1cd42bd4548c3` | GPL-3.0 |
| Rainbow Mode | 1.1.5-rb | `rainbow-1.1.5-rb+804c7785.fwsc` | other | <https://thefirmwarehub.com/fw/rainbow-1.1.5-rb+804c7785.fwsc> · <https://github.com/joshbartlettband-web/felucca-rainbow> | `804c77851c4125f12ce81a50e0f40a421c02401b1a3e72cc4240b231d2e30e83` | GPL-3.0 |
| HYP-1 | 1.5.54 | `hyp1-1.5.54.fwsc` | groove | <https://thefirmwarehub.com/fw/hyp1-1.5.54.fwsc> · <https://github.com/RustyPawz/fm1-HYP1> | `2d310b243ea4f01197339819d4228c8c1f21fe267e7991142e91df527e18f049` | GPL-3.0 |
| LOF-1 | 0.0.87 | `lof1-0.0.87.fwsc` | groove,beta | <https://rustypawz.github.io/fm1-HYP1/lof/firmware/lof1-0.0.87.fwsc> · <https://github.com/RustyPawz/fm1-HYP1/tree/main/lof1> | `dcae62abc37372788502a42438176f81d9aca26588f9b799628ffe7928f88c2d` | GPL-3.0 |
| ECO-1 | 0.0.87 | `eco1-0.0.87.fwsc` | synth,beta | <https://rustypawz.github.io/fm1-HYP1/eco/firmware/eco1-0.0.87.fwsc> · <https://github.com/RustyPawz/fm1-HYP1/tree/main/eco1> | `ca0709fb43cdfb58b2f59a2c9f2e00d40d5441f95f882298524f89d50feac3b1` | GPL-3.0 |
| GRAV-1 | 0.0.87 | `grav1-0.0.87.fwsc` | synth,beta | <https://rustypawz.github.io/fm1-HYP1/grav/firmware/grav1-0.0.87.fwsc> · <https://github.com/RustyPawz/fm1-HYP1/tree/main/grav1> | `393a5d01edea2f4e8a5914fce6863408e384881a3816fa46ff0201ed57469565` | GPL-3.0 |
| CTY-1 | 0.0.87 | `cty1-0.0.87.fwsc` | synth,beta | <https://rustypawz.github.io/fm1-HYP1/cty/firmware/cty1-0.0.87.fwsc> · <https://github.com/RustyPawz/fm1-HYP1/tree/main/cty1> | `effb37ab3439d92597cb28ed2860efdb60390d58836eb8e447d7f577cd6e52b7` | GPL-3.0 |

## About "FM1 Open"

M-VAVE distributes FM1 Open as source plus a browser Workbench, not as a standalone
`.fwsc` of the open application. The latest development kit (V14) ships M-VAVE's official
`FM-1_015.fwsc` (V15) as its base image, and that is the package embedded here and labelled
**FM1 Open V15**. It is the same package the hub offers as the stock/return-to-official
image. Everything else is a community firmware package mirrored by the hub.

## Not embedded

* **Baud Girl**: the hub lists it as a link-only card (it is not open source, so the hub
  may not mirror it); it installs from its maker's own installer on baudgirl.com.

## Notes

* The packages are used unmodified: no patching, no re-signing.
* Only FM-1 (not SMK-37) packages are accepted by the emulator.
* The firmware licences are the authors'; redistributing a binary that contains them means
  complying with GPL-3.0 (everything hub-mirrored) and Apache-2.0 (FM-1_015). The Rust
  application code is separate and licensed MIT.
