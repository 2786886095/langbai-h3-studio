# H3 workflow provenance

The two `h3_*_fixture.json` files in this directory are **project-authored development fixtures**.
They are not official MiniMax or ComfyUI workflows and are not claimed to match a
released H3 node pack. They provide the Studio semantic binding schema and remain
marked `ProjectFixture`.

The `official/` subdirectory contains verbatim Comfy-Org workflow-template assets:

- `video_minimax_h3_t2v.json` — SHA-256 `31ab33fdb053a7834cc866bd7aa08b887518fc656e4a796c89779c6b5e1786e6`
- `video_minimax_h3_r2v.json` — SHA-256 `099d24eda6263854818975c7209db6f29ebfd0339936c928f12293d5ab029ffb`

Source URLs are the corresponding raw files under
`https://github.com/Comfy-Org/workflow_templates/tree/main/templates`.

Those official files are UI workflow graphs, not automatically assumed to be
ComfyUI API-format payloads. Studio keeps them as verified reference assets while
its semantic API adapters remain explicitly project-authored.
