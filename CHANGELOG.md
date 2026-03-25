# Changelog

## [0.8.1](https://github.com/mrtolkien/GHOST/compare/v0.8.0...v0.8.1) (2026-03-25)


### Bug Fixes

* add common Linux fonts to table renderer and bundle DejaVu via Nix ([33758e4](https://github.com/mrtolkien/GHOST/commit/33758e40ccae2c08487aa26e0f736ea556027be9))


### Documentation

* document docling config, vision fallback, and local-first conversion ([4e6f632](https://github.com/mrtolkien/GHOST/commit/4e6f63221fb7aa1fe79240063f9d2a2af9423a33))

## [0.8.0](https://github.com/mrtolkien/GHOST/compare/v0.7.1...v0.8.0) (2026-03-25)

### Features

- add docling module with DoclingDocument serde types and error enum
  ([774148a](https://github.com/mrtolkien/GHOST/commit/774148a34f87a791c7f9da9ba982716145e2b65b))
- add models.vision config for LLM vision fallback
  ([9f2995d](https://github.com/mrtolkien/GHOST/commit/9f2995ddba2c6180a52c3bf123f27a44478cfdc4))
- add render_page.py for PDF page → PNG conversion
  ([4081c88](https://github.com/mrtolkien/GHOST/commit/4081c88bc48f2932faedbb6f0812b023ea5b1da8))
- convert.py outputs DoclingDocument JSON instead of markdown
  ([b16ef64](https://github.com/mrtolkien/GHOST/commit/b16ef648e74ef3138dc54d4e57d4e0f096d28054))
- detect incompatible thinking blocks and show clear error
  ([5c5782f](https://github.com/mrtolkien/GHOST/commit/5c5782f0da3e4628214f9ea98824f5d1c97e7931))
- LLM vision fallback for bad PDF pages
  ([7e3b968](https://github.com/mrtolkien/GHOST/commit/7e3b96833a597359655ba58735e87695f6418b70))
- log file list and reasons in embed sources span
  ([2081779](https://github.com/mrtolkien/GHOST/commit/2081779e61141e69c6c349a1cb214785b1775f6e))
- markdown generation from DoclingDocument tree
  ([5eec757](https://github.com/mrtolkien/GHOST/commit/5eec7577d9c862e4a4f4b87027a1535259fe9cd8))
- per-page quality assessment for DoclingDocument
  ([ee65c99](https://github.com/mrtolkien/GHOST/commit/ee65c99cc951679c94e28bd627ccbd79adaa90dd))

### Bug Fixes

- defer system messages between tool_use and tool_result for Anthropic
  ([de4d50c](https://github.com/mrtolkien/GHOST/commit/de4d50cf28f4392be4e98fd5a197c155f07f3629))
- defer system messages between tool_use and tool_result for Anthropic
  ([a390964](https://github.com/mrtolkien/GHOST/commit/a3909648292710fd4d29819b1c807852e772805a))
- force Anthropic provider in message adjacency tests
  ([90d5eb4](https://github.com/mrtolkien/GHOST/commit/90d5eb48bbc9883095ea1f1ee214b4bb53916252))
- skip system messages when detecting orphaned tool calls
  ([574eb51](https://github.com/mrtolkien/GHOST/commit/574eb5193dac2c986334c788a6b8a779288785d7))
- vision extraction requires system prompt for Codex backend
  ([70813d1](https://github.com/mrtolkien/GHOST/commit/70813d11c348fcd00090c0e9a715e0d2c3137a12))

### Refactoring

- merge nix-shell and services skills into system-management
  ([da02e73](https://github.com/mrtolkien/GHOST/commit/da02e73b2c765087bc496b27dc8611907196fe2c))
- move docling to src/docling/ module, return DoclingDocument JSON
  ([d2d35d7](https://github.com/mrtolkien/GHOST/commit/d2d35d779298ee342e917c1ea2c6983a62d481b0))
- move onboarding templates to src/onboarding/templates/
  ([1a8a383](https://github.com/mrtolkien/GHOST/commit/1a8a3830e9639f9951e7c0678ef12621697c5379))
- move system message relocation to provider-agnostic layer
  ([127a7f3](https://github.com/mrtolkien/GHOST/commit/127a7f3355c825f6774d3bd7425bc0d239510fff))

### Documentation

- hybrid PDF extraction design spec and implementation plan
  ([7bc62e2](https://github.com/mrtolkien/GHOST/commit/7bc62e2c008e3c760f93ba82d4880cc748f12f02))
- update claude code
  ([4bfc707](https://github.com/mrtolkien/GHOST/commit/4bfc707f7707cdc2d81b47bd88dd99a2a5752000))

### Tests

- live tests for Anthropic message adjacency constraints
  ([1031949](https://github.com/mrtolkien/GHOST/commit/103194952d7b68bf43f7aff60381c37731cd68f4))
- live tests for hybrid PDF extraction pipeline
  ([ca8e8e4](https://github.com/mrtolkien/GHOST/commit/ca8e8e4cfc216a67e4a087b6ac5bb92485f8eb20))

## [0.7.1](https://github.com/mrtolkien/GHOST/compare/v0.7.0...v0.7.1) (2026-03-24)

### Bug Fixes

- chunk long components v2 messages
  ([fb11591](https://github.com/mrtolkien/GHOST/commit/fb11591127638f2c5ddd9d35e879368f61d9f5f6))
- preserve SearXNG port bindings on Linux
  ([af46f47](https://github.com/mrtolkien/GHOST/commit/af46f472137c68e6b7ba6dad95928948bee920aa))
- set HOME env var in system-level systemd units
  ([45047e9](https://github.com/mrtolkien/GHOST/commit/45047e97bb95e96e87902ad699c7d2799505945e))

## [0.7.0](https://github.com/mrtolkien/GHOST/compare/v0.6.1...v0.7.0) (2026-03-24)

### Features

- add docling uv conversion script
  ([f8859d5](https://github.com/mrtolkien/GHOST/commit/f8859d5acd0a01a3ff318fab3d2757db5ed399bb))
- add systemd helpers for root/system-level support
  ([2892b10](https://github.com/mrtolkien/GHOST/commit/2892b1021219a26e39e8d28862daf374219d5b70))
- add uv script backend for docling conversion
  ([2dc0ee6](https://github.com/mrtolkien/GHOST/commit/2dc0ee675830c1364f0c41b5ee3ca407fcf2d3a2))
- replace NixNative with Native (uv script) for docling onboarding
  ([0d49b1b](https://github.com/mrtolkien/GHOST/commit/0d49b1b689f90df66a4b1e8272251f78b68f6160))
- support system-level systemd units when running as root
  ([92618e1](https://github.com/mrtolkien/GHOST/commit/92618e1ff0657c4c9b0e386d85a22d170b277381))

### Bug Fixes

- add hostname to traces
  ([3f7f23c](https://github.com/mrtolkien/GHOST/commit/3f7f23c6dcae85f6090b9ab01627be133bcb9548))
- clean up stale docling-serve references and harden uv check
  ([53bee0d](https://github.com/mrtolkien/GHOST/commit/53bee0d262af7a46c57b8aec00c034eb3e4ae8aa))
- generate root-aware systemctl commands in services.toml
  ([bd14a0f](https://github.com/mrtolkien/GHOST/commit/bd14a0fe969b8b7f85aeb2910e790542e36ec4d7))

### Refactoring

- use systemd helpers in onboarding health checks
  ([0299949](https://github.com/mrtolkien/GHOST/commit/0299949225e282b7596bbe3c759f58b6d0221226))
- use systemd helpers in reboot command
  ([8ecebe8](https://github.com/mrtolkien/GHOST/commit/8ecebe80bb7754841b36fe837bb93b43b840b99d))
- use systemd helpers in reload command
  ([3f6c00c](https://github.com/mrtolkien/GHOST/commit/3f6c00c2fbe824aec56dc70197732ece90621caa))
- use systemd helpers in reset command
  ([f30f82f](https://github.com/mrtolkien/GHOST/commit/f30f82f02102344924ea5b2aaf1e50b68c37dd7c))
- use systemd helpers in start/stop commands
  ([dc260c6](https://github.com/mrtolkien/GHOST/commit/dc260c6804150d5a3463675f39c4c581a506e572))
- use systemd helpers in status command
  ([56033f7](https://github.com/mrtolkien/GHOST/commit/56033f7e67f6bb789c3fb4b46e8cb9646f2a7f9a))

### Documentation

- document that podman is a bit of a mess right now
  ([3d548a2](https://github.com/mrtolkien/GHOST/commit/3d548a2479d89b2271d1cc8802eebf7b56e097eb))
- show conversion script path in document import help
  ([09ce70d](https://github.com/mrtolkien/GHOST/commit/09ce70d223ec8155de3f512e35d4aa092b66d58f))

### Tests

- update onboarding tests for Native docling option
  ([dc3e88f](https://github.com/mrtolkien/GHOST/commit/dc3e88f8775446b55411ffebaf48f900ccd8fb91))

## [0.6.1](https://github.com/mrtolkien/GHOST/compare/v0.6.0...v0.6.1) (2026-03-24)

### Bug Fixes

- add link to user guide on first boot
  ([4315d03](https://github.com/mrtolkien/GHOST/commit/4315d039975b6ed0096da9b656af5ef61db51129))

## [0.6.0](https://github.com/mrtolkien/GHOST/compare/v0.5.0...v0.6.0) (2026-03-24)

### Features

- add `ghost skills` CLI command (list, coding, show)
  ([4b03e3b](https://github.com/mrtolkien/GHOST/commit/4b03e3bed7121c35e7e1557e687c59e4bf79e5b9))
- add ChainExhausted variant to ProviderError
  ([3a0621c](https://github.com/mrtolkien/GHOST/commit/3a0621c7440563b0e286b6f046dedbdff7fae855))
- add provider_for_chain and provider_for_model_ref factories
  ([d3d67a5](https://github.com/mrtolkien/GHOST/commit/d3d67a5d129db0d934bf25fcf4ca6214c5ba10ff))
- add StringOrList config type for model chain fallback
  ([86fb810](https://github.com/mrtolkien/GHOST/commit/86fb810fa306e73bd7810283a238e5f17aec9097))
- create skill-creator skill
  ([356cac4](https://github.com/mrtolkien/GHOST/commit/356cac4a1d031b8604b7fab6cd27e8a7b7b47823))
- implement ChainProvider for model fallback chains
  ([a2dc5fc](https://github.com/mrtolkien/GHOST/commit/a2dc5fc53960d643e0dd127b9a7b85415eaaf977))
- show Continue/Stop buttons in Discord on tool iteration limit
  ([5864274](https://github.com/mrtolkien/GHOST/commit/58642740ab070ec2f61f43c81462930320493ca3))
- wire model chain fallback into session and agent creation
  ([982e5b0](https://github.com/mrtolkien/GHOST/commit/982e5b0364ce0e78f719e557b2d53ad8a701f594))

### Bug Fixes

- replace assert! with proper error return in provider_for_chain
  ([2b57710](https://github.com/mrtolkien/GHOST/commit/2b57710ad06154f7d298f7e3bfcee419daf2f1cb))
- skill creator is OF COURSE generic, Claude pls
  ([31313fc](https://github.com/mrtolkien/GHOST/commit/31313fce953a6212a6beb7b809583733b4962a4c))
- tell update to run in the background
  ([e7a2c64](https://github.com/mrtolkien/GHOST/commit/e7a2c64a8e92111d4372dc7faf216b64098b63cf))

### Documentation

- add a short user guide
  ([2698fea](https://github.com/mrtolkien/GHOST/commit/2698feadde6b78c93e516faae0ca836af600abe3))
- add provider integration disclaimer
  ([cf021c6](https://github.com/mrtolkien/GHOST/commit/cf021c6f3f7f4c78477f08ff0f9ff3a6b86d2bd7))
- better index
  ([2b4fa78](https://github.com/mrtolkien/GHOST/commit/2b4fa786559b573fd7fa296d640e10c3571a9555))
- document model chain fallback in providers page
  ([352d9a3](https://github.com/mrtolkien/GHOST/commit/352d9a39a6b90b1347ed1ef46f5e18d14b09a3d5))

### Tests

- integration test for ChainProvider fallback
  ([bb2aac8](https://github.com/mrtolkien/GHOST/commit/bb2aac86d31ab099add6532183eaa62be92962a0))

## [0.5.0](https://github.com/mrtolkien/GHOST/compare/v0.4.0...v0.5.0) (2026-03-23)

### Features

- add ContextOverflow error variant with provider-agnostic detection
  ([860e489](https://github.com/mrtolkien/GHOST/commit/860e489dfff58dd333c93a5c2e2218697ac68caf))
- add ghost services CLI commands
  ([3e6a7b1](https://github.com/mrtolkien/GHOST/commit/3e6a7b19330dc900b6b286d9c0483fc6c8a1e1aa))
- add ghost start/stop commands
  ([9a87e38](https://github.com/mrtolkien/GHOST/commit/9a87e380f0ea3f778625b9cb91255d6eba96246d))
- add ServiceRegistry add/remove/save mutations
  ([054a613](https://github.com/mrtolkien/GHOST/commit/054a6131c821929deccf1cea62e2b1d1866add9f))
- add ServiceRegistry command runner
  ([cfbfd32](https://github.com/mrtolkien/GHOST/commit/cfbfd3280dd656c4d7f2fbfed979d713d224e139))
- add ServiceRegistry type for services.toml parsing
  ([0ee4e03](https://github.com/mrtolkien/GHOST/commit/0ee4e031bccb87c69a3a28943c95301b4c90bbb1))
- catch context overflow errors and retry after compaction
  ([cfbf124](https://github.com/mrtolkien/GHOST/commit/cfbf1242dee23fa788664f856477f9d201d9c28b))
- emit Compacted event from tool loop + Discord notification
  ([a6778e3](https://github.com/mrtolkien/GHOST/commit/a6778e373b453bcc839c2d46ebec16a9da8edf62))
- generate services.toml during ghost init
  ([9d75669](https://github.com/mrtolkien/GHOST/commit/9d756699fa1d45a5e69daa5a12cbed87a3c9dda8))

### Bug Fixes

- remove `document import url`, require download-first workflow
  ([bf2c7b5](https://github.com/mrtolkien/GHOST/commit/bf2c7b5917b7893ffe2c423d30a307f15b100682))
- remove `document import url`, require download-first workflow
  ([66477fc](https://github.com/mrtolkien/GHOST/commit/66477fc3e08718f65708ef1d68542135aa1b7675))
- review fixes — error on missing services.toml, banner ordering, dedupe path helper
  ([63fcef1](https://github.com/mrtolkien/GHOST/commit/63fcef18212ac613bb0a812cdb78eb7e78ffc1a1))

### Refactoring

- ghost reset uses services.toml for shutdown
  ([2e41d30](https://github.com/mrtolkien/GHOST/commit/2e41d3052c1c9e40b0903a16ae0174996d52a30b))
- replace fixed keep_window with dynamic current-turn boundary
  ([c02afea](https://github.com/mrtolkien/GHOST/commit/c02afeac2fce33b733c77e36bc334d03a0be15c6))

### Documentation

- add ghost start/stop/services to CLI reference and services page
  ([db11413](https://github.com/mrtolkien/GHOST/commit/db1141340f394a9facb985d6a1f0bac4ab9fa2e2))
- remove stale keep_window references from configs and docs
  ([d0217f6](https://github.com/mrtolkien/GHOST/commit/d0217f61f51006ce8a92deab74e4e71ff87828f3))
- update services skill with new CLI commands
  ([d59b611](https://github.com/mrtolkien/GHOST/commit/d59b611220dc6f6da4aa2b5513a01366e8b800b0))

## [0.4.0](https://github.com/mrtolkien/GHOST/compare/v0.3.0...v0.4.0) (2026-03-22)

### Features

- add ghost reset
  ([dc3507c](https://github.com/mrtolkien/GHOST/commit/dc3507c1894b624fa4c2a690bf05845c09e21b0d))

### Bug Fixes

- ghost init with existing config actually pre-fills
  ([a8fdb3c](https://github.com/mrtolkien/GHOST/commit/a8fdb3c9db65883ebf83f96314a88174698af35a))
- lots of small onboarding/status fixes
  ([e02ec81](https://github.com/mrtolkien/GHOST/commit/e02ec812ea6c1b2d67bd2e74bed64aa5103aeec3))
- onboarding bugs — compose YAML, llama-server model, health ordering, diff UI
  ([7f9ba3a](https://github.com/mrtolkien/GHOST/commit/7f9ba3a224ede7d4d4d7fea9ba78df713fc172cf))

### Documentation

- Add Contributor Covenant Code of Conduct
  ([c77f68c](https://github.com/mrtolkien/GHOST/commit/c77f68c99feed94d080c3d784866db6b9e141c94))
- Add MIT License to the project
  ([be3bcf8](https://github.com/mrtolkien/GHOST/commit/be3bcf81b3c2b25a49835cd37c1990c8279f6d3f))
- disclaimer + README update
  ([52d2a9f](https://github.com/mrtolkien/GHOST/commit/52d2a9f11a3128b09ab1b1603f38278b05d182b7))
- some wording changes + reviewing TODO for install
  ([7f342e1](https://github.com/mrtolkien/GHOST/commit/7f342e148d696d7c7fd2deced47251a44ad9b747))

## [0.3.0](https://github.com/mrtolkien/GHOST/compare/v0.2.4...v0.3.0) (2026-03-21)

### Features

- add ghost update + mini docs update
  ([9718417](https://github.com/mrtolkien/GHOST/commit/97184176cfa504c36fa0b378afc1817d7febcfeb))
- container onboarding too!
  ([2a1ef19](https://github.com/mrtolkien/GHOST/commit/2a1ef190441a03ad2f5b17654e8ab4c1193073fc))

### Bug Fixes

- add stub src/lib.rs and src/main.rs to deps source
  ([59ae32c](https://github.com/mrtolkien/GHOST/commit/59ae32c6c3548e016735b0d1345246e311c009ee))
- always show HTTP status + full body in provider error messages
  ([440cad1](https://github.com/mrtolkien/GHOST/commit/440cad1db3e7b783c335e17017ea5c46e2bd776a))
- ghost --version also shows hash
  ([1406651](https://github.com/mrtolkien/GHOST/commit/1406651f60053182a43c19b4185a53b8c0b6974f))
- make Codex text_verbosity configurable, default "low"
  ([f4f7e65](https://github.com/mrtolkien/GHOST/commit/f4f7e6528e29e4aa78661fd495cc27a7baf82b6f))
- many small onboard wizard fixes
  ([d028502](https://github.com/mrtolkien/GHOST/commit/d028502a3a225f1711368a0b9c4448b7b102d224))
- remove hardcoded text.verbosity from Codex requests
  ([868058f](https://github.com/mrtolkien/GHOST/commit/868058fd9c05313191c1fffeebfe8e0a6a9d81c6))
- use cargo-only source filter for crane deps to avoid rebuilds
  ([a26c082](https://github.com/mrtolkien/GHOST/commit/a26c082df1d73c31050a0d04182b24b5a001558b))
- use request.system for validation ping, add live tests, fix test breakage
  ([7c38f5d](https://github.com/mrtolkien/GHOST/commit/7c38f5ddf2fc5f35ad02057a0672c6e1a9a51be6))

### Documentation

- nixpkgs note
  ([52da68b](https://github.com/mrtolkien/GHOST/commit/52da68b7282126f9f7c984bf98fdf3c7a3db09b5))
- remove pointless --refresh flag
  ([19e2d1a](https://github.com/mrtolkien/GHOST/commit/19e2d1a62d657b30e04b9b71ddc1047a999c641b))

## [0.2.4](https://github.com/mrtolkien/GHOST/compare/v0.2.3...v0.2.4) (2026-03-20)

### Bug Fixes

- restructure installation.mdx to fix MDX build error
  ([7ee8ed5](https://github.com/mrtolkien/GHOST/commit/7ee8ed5a5922ba6273ac46fe1009d5faf8231cd2))

### Refactoring

- replace ProviderChoice with ProviderKind, use real provider for validation
  ([673a9d7](https://github.com/mrtolkien/GHOST/commit/673a9d75b9152f5f374465f644762b77672855c8))
- use real provider for onboarding agent, delete hand-rolled HTTP
  ([51554da](https://github.com/mrtolkien/GHOST/commit/51554dad7871225fbf373259b1dbbc0e619fae7c))

### Documentation

- add CLAUDE.md rules for using existing abstractions and typed enums
  ([c854685](https://github.com/mrtolkien/GHOST/commit/c8546853572e1fb12978539ad7f9c77c985bf31b))
- update commands with nix add instead of nix install
  ([a185e9c](https://github.com/mrtolkien/GHOST/commit/a185e9cf50d795a9bbf0ddd92f340e6fec8701ed))

## [0.2.3](https://github.com/mrtolkien/GHOST/compare/v0.2.2...v0.2.3) (2026-03-20)

### Bug Fixes

- allow branch names in ghost update --version
  ([10a1a60](https://github.com/mrtolkien/GHOST/commit/10a1a603389508ee621094d262aac5e0df490443))
- disable cachix auto-push daemon, push only final binary
  ([46be7c1](https://github.com/mrtolkien/GHOST/commit/46be7c1bb3160ee4e48aa47df655f1274a65cbd6))

### Documentation

- fix binary cache setup for Determinate Nix
  ([45b9d4e](https://github.com/mrtolkien/GHOST/commit/45b9d4e7217dfaf9e845505d58b1f49d5a54d225))

## [0.2.2](https://github.com/mrtolkien/GHOST/compare/v0.2.1...v0.2.2) (2026-03-19)

### Bug Fixes

- only rewrite ghost entry in Cargo.lock, not all packages
  ([dd6a27f](https://github.com/mrtolkien/GHOST/commit/dd6a27f0f78f82b651c7ede4be176a224979015f))
- stabilize crane deps cache across version bumps, use v\* tags
  ([fb58a5c](https://github.com/mrtolkien/GHOST/commit/fb58a5c38532a11763e89edaa24c29903eb86c09))
- use separate dep source in crane to avoid 11-min cache busts
  ([8d5d891](https://github.com/mrtolkien/GHOST/commit/8d5d891605169f405e89014042b4daa990986b47))

## [0.2.1](https://github.com/mrtolkien/GHOST/compare/ghost-v0.2.0...ghost-v0.2.1) (2026-03-19)

### Bug Fixes

- trigger on properly named tags!
  ([4ddc1bf](https://github.com/mrtolkien/GHOST/commit/4ddc1bf487fd6fa6074fce9ccb0a7e14479874a5))

## [0.2.0](https://github.com/mrtolkien/GHOST/compare/ghost-v0.1.0...ghost-v0.2.0) (2026-03-19)

### Features

- accessibility tree parsing, ref assignment, and XML rendering
  ([8328d7a](https://github.com/mrtolkien/GHOST/commit/8328d7a261295303fd819e6e59d18039c39abadf))
- add --ref flag to git reference import for branch/tag targeting
  ([6653937](https://github.com/mrtolkien/GHOST/commit/665393791b2416f8d6888db757925bef72dfea85))
- add [docling] config section, remove [web].docling_url
  ([4ef2e26](https://github.com/mrtolkien/GHOST/commit/4ef2e26b26cec604c576255b45ad226fbdc87162))
- add /feedback command for Discord
  ([15fbb31](https://github.com/mrtolkien/GHOST/commit/15fbb313129ee73a11cd475e6b53e47c6edcd8a1))
- add \`ghost config reload\` CLI command
  ([b281627](https://github.com/mrtolkien/GHOST/commit/b281627f58356b5d24f6997cd3ea0dc87edb87cd))
- add `ghost reboot` command for graceful daemon restart
  ([aa8d1c1](https://github.com/mrtolkien/GHOST/commit/aa8d1c1dc82baa634c79048f6ef3ab33f4db65a0))
- add Archetype enum and new NoteFrontMatter fields
  ([42cfd50](https://github.com/mrtolkien/GHOST/commit/42cfd50d3c98ede83723cf298e65d074c26f6aaa))
- add atomic session guard to prevent concurrent tool loops
  ([d50a472](https://github.com/mrtolkien/GHOST/commit/d50a472a8732c73f64b61c01ca1029e25303141d))
- add batch citation check for orphan detection
  ([4930e7f](https://github.com/mrtolkien/GHOST/commit/4930e7ff0af10af3c5681493f51ea34fc337170f))
- add boot_with_config() and LiveTestEnv::boot_daemon()
  ([1ce19ed](https://github.com/mrtolkien/GHOST/commit/1ce19edac221943092e218c479ac30b44a260175))
- add browser tool display formatting for Discord
  ([7e1478b](https://github.com/mrtolkien/GHOST/commit/7e1478b0e84ef43d7e0e936ac29b217b4566d5db))
- add browser-use skill, slim tool description
  ([a74c925](https://github.com/mrtolkien/GHOST/commit/a74c925fad5421185210820913e5f5b0b0dcee1f))
- add BrowserError enum for browser tool
  ([db1266a](https://github.com/mrtolkien/GHOST/commit/db1266a760831c616daacd3e7d214f8ff34b6342))
- add bulk file hash loading queries for boot reconciliation
  ([8542bf9](https://github.com/mrtolkien/GHOST/commit/8542bf9ec9977add659d93e819fdfc29cc223f10))
- add bundled file diff, change detection, and update application logic
  ([ad4df4c](https://github.com/mrtolkien/GHOST/commit/ad4df4c30bc0efa84e1eea13c9751b373cdf4676))
- add busy counters to subsystems for settle() support
  ([562dc7f](https://github.com/mrtolkien/GHOST/commit/562dc7f3eecf5f455ace91d6b51a2afb2638c715))
- add button and action row v2 component builders
  ([15b8af7](https://github.com/mrtolkien/GHOST/commit/15b8af761bd7b421a20b9466e22a11999afd9eea))
- add Cachix binary cache for instant nix installs
  ([776aec9](https://github.com/mrtolkien/GHOST/commit/776aec966462dad67d2078ab3257b5608497e602))
- add chrome_cdp_url to web config
  ([68cb4af](https://github.com/mrtolkien/GHOST/commit/68cb4af02d646508693399938ca46c593d71f4a3))
- add citation format instructions to system prompt
  ([21703f6](https://github.com/mrtolkien/GHOST/commit/21703f61e5441f79bb4bbd5f9579f01af383547a))
- add Claude Code tool name translation for Anthropic provider
  ([4d2f6cf](https://github.com/mrtolkien/GHOST/commit/4d2f6cfc2bccd524f249618dfeff988c566e2ce3))
- add code category and repo filter to knowledge_search tool
  ([b03eb1f](https://github.com/mrtolkien/GHOST/commit/b03eb1f27254fe79441de7472a803d7827e6e5a7))
- add code repo reconciliation with gitignore walk and reverse-pass cleanup
  ([4480257](https://github.com/mrtolkien/GHOST/commit/448025728618305c9a7e11c64f6caedf08c7d795))
- add code search and lib docs guidance to coding agent prompt
  ([fcaf39d](https://github.com/mrtolkien/GHOST/commit/fcaf39dcea9491d7bdb5bb2396f0570e7d650099))
- add code_file CRUD and hash loading functions
  ([955e0f2](https://github.com/mrtolkien/GHOST/commit/955e0f2f3fe9a28ca4e52c34365b192ae8c6a4c6))
- add code_file table, FTS5, and CodeFileRecord
  ([ad9abd0](https://github.com/mrtolkien/GHOST/commit/ad9abd0aecc9d07cbb04d3a3be585f4bb785739d))
- add coding session lookup by chat session ID
  ([e138802](https://github.com/mrtolkien/GHOST/commit/e138802ed4849da2197931bd4922a2522e8bada3))
- add coding-implementer Lua agent
  ([96b2cc3](https://github.com/mrtolkien/GHOST/commit/96b2cc3a3e48791b092ef2165b47e1b864daef43))
- add coding-quality-reviewer Lua agent
  ([2c235bb](https://github.com/mrtolkien/GHOST/commit/2c235bb17416f52c763c334f7f146519bcc47ddf))
- add coding-reviewer Lua agent for final code review
  ([18887ad](https://github.com/mrtolkien/GHOST/commit/18887ad7b55688343d1593d695a1bb5e9bfa2979))
- add coding-spec-reviewer Lua agent
  ([31b24e7](https://github.com/mrtolkien/GHOST/commit/31b24e70e7633623a170697122cd84e46ec88007))
- add collect_extras() for skill companion file discovery
  ([c9a95e8](https://github.com/mrtolkien/GHOST/commit/c9a95e868e467c25c2fc550a8430bf91ae4c82e7))
- add compose templates and SearXNG config to templates/
  ([e6f4ffc](https://github.com/mrtolkien/GHOST/commit/e6f4ffca256e99de628ad6294c7750c56258c0ae))
- add config generation and diff display to onboarding
  ([593e5ff](https://github.com/mrtolkien/GHOST/commit/593e5ff1d60165c245c64323248b57054c62c464))
- add config hot reloading with ghost config reload
  ([947442b](https://github.com/mrtolkien/GHOST/commit/947442b6cd4b41014cf245ebaa9e9bdcbf38ff9f))
- add config write-back for browser management (add/remove)
  ([28f8cb7](https://github.com/mrtolkien/GHOST/commit/28f8cb7eaaa2d08a996d35e87f75dd53858f6a56))
- add Confirmation type for tool dialogues
  ([49dba77](https://github.com/mrtolkien/GHOST/commit/49dba7778587855914b5417dd8fa95b893655a46))
- add ContentBlock::Image variant
  ([4f0fba9](https://github.com/mrtolkien/GHOST/commit/4f0fba9b8f8c4fbbc63e056ff959961d655f4d1f))
- add ContentBlock::Thinking variant for typed reasoning blocks
  ([7299b2e](https://github.com/mrtolkien/GHOST/commit/7299b2e25159e237be5c8794405bfcc7f0e17256))
- add ctx:call_tool() and ctx:call_tools() for Lua agent hooks
  ([d9aa829](https://github.com/mrtolkien/GHOST/commit/d9aa829d148e1b5d8a2e3713a25f47aec3c0edb6))
- add Discord setup to onboarding wizard
  ([9c9fbde](https://github.com/mrtolkien/GHOST/commit/9c9fbde94e2eb34b6f99b9cb3eac811550743701))
- add document-import skill, update reference-import for git/crawl only
  ([33a3d5b](https://github.com/mrtolkien/GHOST/commit/33a3d5b0903de523a82b8791536a4664c3598322))
- add environment detection for onboarding wizard
  ([bb104d8](https://github.com/mrtolkien/GHOST/commit/bb104d8e11de56d462a40057026d11e116d36eb5))
- add file_hash column to knowledge tables
  ([88da617](https://github.com/mrtolkien/GHOST/commit/88da61739b28ace4d63136f6613c5dc34d08eece))
- add ghost browsers CLI — list, add, remove, discover, check
  ([666d81c](https://github.com/mrtolkien/GHOST/commit/666d81cb669d088f6f16a9aa0440dd23dbcc211a))
- add ghost browsers serve — start browser with Tailscale CDP relay
  ([0adc397](https://github.com/mrtolkien/GHOST/commit/0adc39739d0b3af35b6a464b859d02778202a239))
- add ghost reference update CLI command
  ([9560303](https://github.com/mrtolkien/GHOST/commit/9560303b62cf914b22f7e5fc44f46864743ff302))
- add ghost send-image and ghost attach CLI commands
  ([4c7c0dc](https://github.com/mrtolkien/GHOST/commit/4c7c0dc26ec6e5db9a545196ce9bfc1badf1d160))
- add ghost update command wrapping nix profile
  ([e0d59cc](https://github.com/mrtolkien/GHOST/commit/e0d59ccc535eafe29137999abc7aaf68ab279d8f))
- add has_run_since DB query helper
  ([b67d8c2](https://github.com/mrtolkien/GHOST/commit/b67d8c28d2efcf4e39d0624f24a3de34049a27f5))
- add health checks and service launcher to onboarding
  ([34aee4e](https://github.com/mrtolkien/GHOST/commit/34aee4e520942beb73adbb2212ba5b59f617461b))
- add hourly periodic embedding reconciliation
  ([d5142e5](https://github.com/mrtolkien/GHOST/commit/d5142e570a1335a2bd8c1a84a731509e22307dd3))
- add ignore crate and code-specific walking utilities
  ([74f627d](https://github.com/mrtolkien/GHOST/commit/74f627dc6f3694da46ac271c42a2893287f4261b))
- add image crate dependency for vision support
  ([3d8215a](https://github.com/mrtolkien/GHOST/commit/3d8215a6b5f6e6825d00d06c96ab0bc02b706d23))
- add image generation
  ([7b2e47e](https://github.com/mrtolkien/GHOST/commit/7b2e47e0461c358f62f0022dcdf912911614f36b))
- add image utility module (load, compress, mime detection)
  ([c3d57d1](https://github.com/mrtolkien/GHOST/commit/c3d57d1f0334a78abfa6198863c179e9638a0011))
- add import_config JSON column to import_batch
  ([22f8604](https://github.com/mrtolkien/GHOST/commit/22f86041a99e172deae1dfaed9da7981f06a23d3))
- add interactive onboarding wizard
  ([3cc5299](https://github.com/mrtolkien/GHOST/commit/3cc529936bb6dd87a250cb65de3cbe6636b5299b))
- add interruptions/steering for tool loops
  ([1a20881](https://github.com/mrtolkien/GHOST/commit/1a20881b83f1ece6618d1cda353b76b15c1d6af3))
- add JSX support to code chunker
  ([608d151](https://github.com/mrtolkien/GHOST/commit/608d151355b79cf914ac1a02f0e14fc662ef1c48))
- add last_message_at DB query helper
  ([efbd82d](https://github.com/mrtolkien/GHOST/commit/efbd82dc083f602eeb45ea728a5afa4a4f91e07c))
- add launchd plist templates for llama-server and docling-serve
  ([8ae0825](https://github.com/mrtolkien/GHOST/commit/8ae0825e4912b084c56d90f0cdf51957a4d07222))
- add line-based three-way merge algorithm
  ([c4165cf](https://github.com/mrtolkien/GHOST/commit/c4165cf880d925a95e3d718cf2e51f98164fb6f3))
- add macOS install script
  ([3bd3144](https://github.com/mrtolkien/GHOST/commit/3bd3144c5ae1bc09cf83597007004cff1ce42ff7))
- add macOS-specific docker-compose for installed setup
  ([fad4134](https://github.com/mrtolkien/GHOST/commit/fad4134bd5e0e9b91bb19a424f98fb330dc9b66e))
- add manual trigger channel for idle agents
  ([9c6d430](https://github.com/mrtolkien/GHOST/commit/9c6d43015ae5d2d543172021be6c756e5d1af4a8))
- add message_source CRUD and backfill queries
  ([6fa8118](https://github.com/mrtolkien/GHOST/commit/6fa811871edf8bbc0c0f8c2f4c0480d097a8354f))
- add message_source table for message-to-reference linking
  ([7c5cfda](https://github.com/mrtolkien/GHOST/commit/7c5cfda4d8fef34b01bf478fdf2ad827e8c83e87))
- add on-demand onboarding assistant
  ([c78c2d4](https://github.com/mrtolkien/GHOST/commit/c78c2d4a6044570eef6f2601b7adf187407d9bec))
- add onboarding module skeleton and dependencies
  ([6e2cd92](https://github.com/mrtolkien/GHOST/commit/6e2cd92c4fb767bf5e41a5a822368d23ec590fb2))
- add press, hover, select, fill, wait, evaluate, drag, resize actions
  ([6e8adfa](https://github.com/mrtolkien/GHOST/commit/6e8adfa51007e05d6e8abe4e39937e99a7b52ed1))
- add press, hover, select, fill, wait, evaluate, drag, resize browser actions
  ([b54f747](https://github.com/mrtolkien/GHOST/commit/b54f74711547859781ff5bd8e22ee90b3d7748bd))
- add provider selection and validation to onboarding
  ([237c6b8](https://github.com/mrtolkien/GHOST/commit/237c6b85d9e9eb7e76cf7572209d3c65d42df060))
- add script table, FTS5 index, and sync triggers
  ([853ff20](https://github.com/mrtolkien/GHOST/commit/853ff20016d20ac71466e9500afd234341091351))
- add ScriptRecord type
  ([672cf42](https://github.com/mrtolkien/GHOST/commit/672cf42414cecaf843ca7b1e07843a93b1983488))
- add search_code_files FTS5 search with repo filtering
  ([d5bcb18](https://github.com/mrtolkien/GHOST/commit/d5bcb1867d5572c1bc929f1c4f0272b2a528b9fd))
- add sending-attachments skill
  ([21baea0](https://github.com/mrtolkien/GHOST/commit/21baea0865cace72b6fbae3220a0fc9e1455d4f0))
- add service file generation, move utils from init.rs
  ([f555ec2](https://github.com/mrtolkien/GHOST/commit/f555ec2cb40329476360a5fcaf8b72126c96d061))
- add service setup and compose generation to onboarding
  ([7566742](https://github.com/mrtolkien/GHOST/commit/75667427d674049382dc461b4358c32ee452081c))
- add services skill with observability and tailscale extras
  ([436c1e9](https://github.com/mrtolkien/GHOST/commit/436c1e90bec3fc38b25d6cc689cbdb054db19693))
- add session event bus types (src/events.rs)
  ([0196ed4](https://github.com/mrtolkien/GHOST/commit/0196ed46ef24ab8a3186edb48a7bf5b5b08547dd))
- add SharedConfig type, reload function, and immutable field validation
  ([08a3f5f](https://github.com/mrtolkien/GHOST/commit/08a3f5f6b54a0d3849fab8c3ee8cfe7d93de0ce5))
- add SigNoz Docker Compose stack for self-hosted observability
  ([d4e7fd2](https://github.com/mrtolkien/GHOST/commit/d4e7fd211cacda6764d9a3fb9c83c9d80cf7f74f))
- add source field to skill frontmatter
  ([4588319](https://github.com/mrtolkien/GHOST/commit/458831978d30aff2c1f7ffa3d9ec45677aaaca12))
- add ToolDisplay module with request/result summaries for all tools
  ([79d637e](https://github.com/mrtolkien/GHOST/commit/79d637ef010f460efe7112d64cff99369f3b9cbd))
- add transactional replace_embeddings_for_source
  ([b35dcd9](https://github.com/mrtolkien/GHOST/commit/b35dcd98c3b320afb1009bc4f043fc2d7beb9000))
- add unified session event handler
  ([ac3d295](https://github.com/mrtolkien/GHOST/commit/ac3d2956ce59103d7f64fa1e538843476027c2da))
- add uploads to browser
  ([8032831](https://github.com/mrtolkien/GHOST/commit/8032831a04ed1ac138e3800dbdfaaee8201d4083))
- add verbosity=low for Codex Responses API and xhigh reasoning effort
  ([528fef0](https://github.com/mrtolkien/GHOST/commit/528fef092cd6b712f8e93b21db41b14d047674d6))
- add workflow guidance to browser tool description
  ([a0a777c](https://github.com/mrtolkien/GHOST/commit/a0a777c1aec98ad94a336220a1e29171a80585b5))
- AgentRunner sends SessionEvent on agent completion
  ([882450e](https://github.com/mrtolkien/GHOST/commit/882450ede7a887e3b2fca22b8ea39307a0f6c7f6))
- allow agents to live inside skill directories
  ([80ff8a3](https://github.com/mrtolkien/GHOST/commit/80ff8a35b9fc63784c7b57ced46549e45fc7a059))
- anthropic credential reading and OAuth token refresh
  ([0f65254](https://github.com/mrtolkien/GHOST/commit/0f6525424e3238d27a4001d285c5fbd4a2082099))
- anthropic message conversion (Ghost → Anthropic Messages API)
  ([4d5dbf4](https://github.com/mrtolkien/GHOST/commit/4d5dbf45ee456ca64e4a99d66d395589bc826668))
- anthropic SSE stream parser
  ([cc26dcc](https://github.com/mrtolkien/GHOST/commit/cc26dccab1171cb4893336794a5adc16a8b867fd))
- Anthropic streaming parser produces ContentBlock::Thinking
  ([46c1c59](https://github.com/mrtolkien/GHOST/commit/46c1c5911a5933694d182b2dc9e4ecd7c2cc7ee3))
- AnthropicProvider struct, trait impl, and registration
  ([d0697d5](https://github.com/mrtolkien/GHOST/commit/d0697d5312e254fb2c7a367bda3152ef4033449d))
- backfill message_source.reference_id during curation
  ([c83efe0](https://github.com/mrtolkien/GHOST/commit/c83efe044c983572bd52b4d84a83c3fa8310e8e6))
- batch hash-check in reconcile_filesystem, skip unchanged files
  ([c1eae8e](https://github.com/mrtolkien/GHOST/commit/c1eae8eb8e42e6228af6c95cd626bb2c65cb076a))
- browser integration tests and raw JSON AX tree parsing
  ([93fb3e8](https://github.com/mrtolkien/GHOST/commit/93fb3e840f1a8b0c0fb7188c8079f4673117c239))
- browser tool with ToolContext integration
  ([1614d74](https://github.com/mrtolkien/GHOST/commit/1614d741cea34b382946f121a2b8c4826f81230c))
- **browser:** add browser/tab context to tool output, finalize Crawl4AI integration
  ([6c79224](https://github.com/mrtolkien/GHOST/commit/6c79224da77fd964f5beb4a82734800c51b6814e))
- **browser:** add BrowserManager, replace BrowserSession
  ([18692e5](https://github.com/mrtolkien/GHOST/commit/18692e58170ba78373b0d63ee56b9bfb1472a19b))
- **browser:** add CDP discovery — localhost + Tailscale peer scanning
  ([46d6b65](https://github.com/mrtolkien/GHOST/commit/46d6b65f6a4e204e54ca86b0520cd9f259ccc322))
- **browser:** add multi-browser error variants
  ([440a367](https://github.com/mrtolkien/GHOST/commit/440a367351b65c354f4f626b858d51c056ab85f5))
- **browser:** add multi-browser management — browsers, connect, disconnect
  ([872be38](https://github.com/mrtolkien/GHOST/commit/872be3857bb379b2c13ff23b09cf80b23df5e082))
- **browser:** add multi-tab support — open, focus, close, tabs actions
  ([f7e3ce7](https://github.com/mrtolkien/GHOST/commit/f7e3ce7469b52173e8dce5b83c6d2f2d9821aa94))
- BrowserSession API with SSRF protection
  ([873b8fa](https://github.com/mrtolkien/GHOST/commit/873b8fa33256a85ee5b25a42722adf9a0cf63868))
- BrowserSession with navigate, snapshot, click, type, scroll, screenshot
  ([972a4f7](https://github.com/mrtolkien/GHOST/commit/972a4f70424f4c688ae62c15aed29f4815cabddd))
- bundled scripting skill for GHOST
  ([2e0837f](https://github.com/mrtolkien/GHOST/commit/2e0837fc00cba7916c53eacf725996c409d556e6))
- CDP connection and page action primitives
  ([e6f5e1c](https://github.com/mrtolkien/GHOST/commit/e6f5e1cbf9965248489fa9f062b24bf9b1774599))
- change DEFAULT_SKILLS to multi-file DefaultSkill struct
  ([0ea2bfc](https://github.com/mrtolkien/GHOST/commit/0ea2bfc10c60ad67613667ed7bfd8d6706d5106e))
- CI with native binary build + nix Docker image
  ([0ca3ab4](https://github.com/mrtolkien/GHOST/commit/0ca3ab4a3fc0c05096c8d49af918845c10b74da9))
- clean citation sections for compact Discord display
  ([957f127](https://github.com/mrtolkien/GHOST/commit/957f1277d42054ea51cbe98ec2ffe1f7302bd30c))
- Codex + OpenAI providers consume ContentBlock::Thinking
  ([a3b08a0](https://github.com/mrtolkien/GHOST/commit/a3b08a03c95cf5d53f523258ada5f65349675003))
- Codex Responses parser produces ContentBlock::Thinking for reasoning
  ([d2526e5](https://github.com/mrtolkien/GHOST/commit/d2526e54866dde71eef16fc7f71227d4fca3f5df))
- convert knowledge-navigator skill to folder with SQL schema extra
  ([172f368](https://github.com/mrtolkien/GHOST/commit/172f3681aea6cabb7c9a352c660b8f20f3442a77))
- core reference update logic with diff and orphan protection
  ([1c285fb](https://github.com/mrtolkien/GHOST/commit/1c285fbc1ae5c7faf7823940923478ec7ff83945))
- crawl4ai as primary HTML path with HEAD routing and agent options
  ([768852b](https://github.com/mrtolkien/GHOST/commit/768852b3a8c80bce5bbdd29674ed0680420a2e45))
- create scripts/ directory on workspace bootstrap
  ([90b0b5f](https://github.com/mrtolkien/GHOST/commit/90b0b5f5c4d1f098e4ded4332eca1739f23fd8aa))
- DB round-trip + chat layer support for ContentBlock::Thinking
  ([0306b55](https://github.com/mrtolkien/GHOST/commit/0306b557b7a7caeb122a5e4dcced1befbf91859c))
- Discord image attachments passed as image content blocks
  ([d24eed3](https://github.com/mrtolkien/GHOST/commit/d24eed3d6eda652c065f6a9b7795f58d515e4b6a))
- **docs:** add matrix rain hero with glitch + CRT effects
  ([39e81c7](https://github.com/mrtolkien/GHOST/commit/39e81c7041c81b096c7bf507fd15d0c18bae8475))
- **docs:** rewrite CSS to Cold Terminal / Tokyo Night aesthetic
  ([ef877c6](https://github.com/mrtolkien/GHOST/commit/ef877c66b11c1ba3d9e41e1a5fd1a91fef1082f4))
- embed docs in binary and install to references/ghost/docs/ on boot
  ([9285449](https://github.com/mrtolkien/GHOST/commit/9285449efe6105ff9b98a48e4185d563e69a059b))
- embed git commit hash at build time
  ([f8dc2d1](https://github.com/mrtolkien/GHOST/commit/f8dc2d1f2f3fd76c457bf287ffd413d40fa73ccc))
- embedding reconciliation for scripts
  ([c90fd63](https://github.com/mrtolkien/GHOST/commit/c90fd635b0fcad7f79331e95f150f29d68323cdb))
- exclude notes/.archive/ from indexing, search, and embeddings
  ([aff1920](https://github.com/mrtolkien/GHOST/commit/aff1920e4a3d62c2d39ad8c1c37255f1e82831c5))
- expand directory paths in watcher to fix inotify race
  ([acf4a28](https://github.com/mrtolkien/GHOST/commit/acf4a288e094599a3ea30155f3fb0fd32a50b62d))
- extend ToolLoopEvent with result phase and display strings
  ([478c52d](https://github.com/mrtolkien/GHOST/commit/478c52d0234c4e8e6d510033d78c7b2d3b5465b8))
- extract citations from responses and create message_source records
  ([fd3317e](https://github.com/mrtolkien/GHOST/commit/fd3317e0ead00f43da214e8fc300357bb41a7a75))
- file_edit ask_for_validation sends confirmation via channel
  ([d1cde12](https://github.com/mrtolkien/GHOST/commit/d1cde1279ddfdbbb4794ed789f6006dd254c9683))
- filesystem reconciliation discovers files missed by watcher
  ([ee766af](https://github.com/mrtolkien/GHOST/commit/ee766af2a10407997862378e34194b03b9c1364d))
- filesystem watcher support for scripts/
  ([ca15396](https://github.com/mrtolkien/GHOST/commit/ca1539628f892f33f8ed8865c351d6cd2f654dec))
- fix the move to home manager: now working!
  ([db3cf57](https://github.com/mrtolkien/GHOST/commit/db3cf57b82335762d03658c442d551272737e022))
- flake.nix builds ghost from source via buildRustPackage
  ([fda4721](https://github.com/mrtolkien/GHOST/commit/fda4721309c166e5682dd7b1942aabca64079fd1))
- ghost init generates systemd/launchd service files
  ([8fca1a2](https://github.com/mrtolkien/GHOST/commit/8fca1a2dd57c27d998ddcdf0c4945500d8947c97))
- ghost shell rebuild for hot-swapping nix shell env
  ([14366e5](https://github.com/mrtolkien/GHOST/commit/14366e51eb3b26b5dc7a55220b415ccdb41bddbd))
- ghost version prints git commit hash
  ([6c2005c](https://github.com/mrtolkien/GHOST/commit/6c2005c613e0883ba362eac859a36d80ae3e7f8b))
- gracefully skip Discord when bot token is missing
  ([92c6122](https://github.com/mrtolkien/GHOST/commit/92c6122735dd2972d8835e70e4b155a3d2cdd718))
- guide GHOST to reference-import skill on unsupported content types
  ([e2489ab](https://github.com/mrtolkien/GHOST/commit/e2489abeacf7f516f24e1185d641a86dfd4c313b))
- handle Discord component interactions for button clicks
  ([5624538](https://github.com/mrtolkien/GHOST/commit/56245381b876a6b2768532c454fe5715d5f7a111))
- handle SessionBusy in event handler and Discord bot
  ([cfd33c2](https://github.com/mrtolkien/GHOST/commit/cfd33c2699063cee46826d54392c382e7aed2346))
- increase max image dimension to 2048px for better visual understanding
  ([49a4ed4](https://github.com/mrtolkien/GHOST/commit/49a4ed47b98a3ccbab4c98e8416f25028b2b90e0))
- knowledge_search supports archetype filter parameter
  ([2530fd8](https://github.com/mrtolkien/GHOST/commit/2530fd89cbd5043705288aafa4a17983ee8dacd4))
- knowledge_search supports scripts category
  ([ff45934](https://github.com/mrtolkien/GHOST/commit/ff45934e185a4c7648fecb7f7553cdfeb82c32c8))
- live flake includes ghost binary, CI commits flake.lock to main
  ([97033cb](https://github.com/mrtolkien/GHOST/commit/97033cb245557022257e644af735cb849433167f))
- load last 2 diary entries into system prompt
  ([99066f8](https://github.com/mrtolkien/GHOST/commit/99066f8ea573e1ce98b0bced3ebb551dfcbdda21))
- make discover_skills recursive via walk_skills_dir
  ([ec5660e](https://github.com/mrtolkien/GHOST/commit/ec5660ef9ae75e008e1e20ab94f99efc05bf9cf4))
- mask old Image blocks in compaction Phase 1
  ([f766ee9](https://github.com/mrtolkien/GHOST/commit/f766ee9467bddf17a4b9d8e6d870874f3bced29f))
- migrate Discord handler, wire SharedConfig through daemon, add SIGHUP handler
  ([f387686](https://github.com/mrtolkien/GHOST/commit/f38768695457610e9fe314aff9716634d16335d8))
- minimal nix runtime Dockerfile + smart entrypoint
  ([bcf9789](https://github.com/mrtolkien/GHOST/commit/bcf9789982029a968525141554c450c5515da582))
- note_write archive action moves note to .archive/ and removes from index
  ([cee0392](https://github.com/mrtolkien/GHOST/commit/cee03922cc7665630bcc40459004acb09aed7eb5))
- note_write rejects notes with unverified source URLs
  ([d1a9288](https://github.com/mrtolkien/GHOST/commit/d1a92880a910179b74ce239919c7f39378b0221e))
- note_write supports archetype, parent, timestamps
  ([55cebce](https://github.com/mrtolkien/GHOST/commit/55cebce2cabe2825452218ea195bc208d09df243))
- OpenAI-compatible provider image support
  ([edce49b](https://github.com/mrtolkien/GHOST/commit/edce49ba9e4064f1574b02e5e126eadea58da796))
- parse home-manager package list in system prompt
  ([0fac944](https://github.com/mrtolkien/GHOST/commit/0fac944b16a076351e946ecc66235e80ed713852))
- pass GHOST_SESSION_ID env var to shell child processes
  ([b68d3a7](https://github.com/mrtolkien/GHOST/commit/b68d3a7c966158d7bb0e413d24ed0495e0e3145c))
- persist full import config in \_import.toml and import_batch
  ([749cfef](https://github.com/mrtolkien/GHOST/commit/749cfef8a5b6ded7fe0f3ab4ce58cd7c1c44d3c4))
- plumb confirmation_tx through ToolContext
  ([6f9c6ca](https://github.com/mrtolkien/GHOST/commit/6f9c6ca443c6f64700dfb4828bb15e998e36bfdb))
- plumb docling options (no_ocr, page_range) through import types
  ([ca372fd](https://github.com/mrtolkien/GHOST/commit/ca372fd451f1c501ba89f676d4225e1d5d2b55f0))
- plumb file_hash through knowledge CRUD layer
  ([2d63a2e](https://github.com/mrtolkien/GHOST/commit/2d63a2e5b2d8b2bd7ab6c5b516e0e85cea9b3bed))
- port companion files for writing-skills, requesting-review, tdd
  ([130239d](https://github.com/mrtolkien/GHOST/commit/130239db5788f995706a10ae62c026b9e4d148de))
- port subagent-development companion files (implementer, reviewers)
  ([92a0a45](https://github.com/mrtolkien/GHOST/commit/92a0a457e336634df66f2b49095dbf054708c9fe))
- port systematic-debugging companion files
  ([42060f7](https://github.com/mrtolkien/GHOST/commit/42060f7e9f5a1ad47ef7a65446d6246732a394b3))
- prompt user to review bundled file updates on boot via Discord
  ([b22d69e](https://github.com/mrtolkien/GHOST/commit/b22d69e315245d37f79013f3be602cc6717f2eab))
- re-introduce archetype column in DB, update CRUD signatures
  ([bc442d6](https://github.com/mrtolkien/GHOST/commit/bc442d6fbfac3a4511ff06bd7e24eb24e143b0f4))
- read back stored import config for replay
  ([63c4fb9](https://github.com/mrtolkien/GHOST/commit/63c4fb9f42926037768eb19bf828fd2e85c32ec7))
- read_file appends &lt;extra-files&gt; block for skill.md
  ([7cd4dfa](https://github.com/mrtolkien/GHOST/commit/7cd4dfa98f290cae581d6c33825fb30e06e97d8f))
- read_file returns images for image files
  ([719f7d0](https://github.com/mrtolkien/GHOST/commit/719f7d03837592a9144a9d781f895e32647a5c06))
- reconcile_edges injects parent edge, stubs use entity archetype
  ([6c07d38](https://github.com/mrtolkien/GHOST/commit/6c07d38ed8cd5ee26f88c6e9c98d94545408f620))
- redesign statusline with separator, subtext, and tool emojis
  ([ddf17fb](https://github.com/mrtolkien/GHOST/commit/ddf17fbdb42f67595e53b722f4d52abcf291ae92))
- register /stop, /reboot, /kill as Discord slash commands
  ([9be03d1](https://github.com/mrtolkien/GHOST/commit/9be03d1aa08dca3d9638667ba9f13452bcb3044f))
- register coding Lua agents as default agents
  ([ced6a22](https://github.com/mrtolkien/GHOST/commit/ced6a2257cd97e927477819a4abe0d284355e6a2))
- render skill source in prompt context
  ([edb4928](https://github.com/mrtolkien/GHOST/commit/edb49285459b748f79bbbacd1a5925c38e469e08))
- replace chrome_cdp_url with [[web.browsers]] config array
  ([9d67051](https://github.com/mrtolkien/GHOST/commit/9d67051e0ba6f5430693d66318f67d5910efdcb0))
- replace nix develop wrapping with home-manager profile PATH
  ([ec0ab57](https://github.com/mrtolkien/GHOST/commit/ec0ab577855dc016470bada2643b8095dac34a3d))
- rewrite docling client to use async API with polling
  ([0e2a91f](https://github.com/mrtolkien/GHOST/commit/0e2a91fbe76780945a6100fc6b899db03d4c5df5))
- rewrite ghost init with CLI flags and detection phase
  ([659982b](https://github.com/mrtolkien/GHOST/commit/659982b5cdcaebb42d21d99a9b7bdf794438e629))
- rewrite observability pipeline -- logfire -&gt; standard OpenTelemetry
  ([e6b70c3](https://github.com/mrtolkien/GHOST/commit/e6b70c38a8217ce8520921457f599b4097c182e2))
- root nix flake for ghost binary package
  ([85655d4](https://github.com/mrtolkien/GHOST/commit/85655d4563313e06a75c943955ea53561cbf4d5c))
- run home-manager switch in entrypoint for workspace toolchain
  ([8f64f54](https://github.com/mrtolkien/GHOST/commit/8f64f54e149c07dea27ace7fac6a72cdd143b956))
- scheduler reacts to config reload for tick interval changes
  ([f67f26e](https://github.com/mrtolkien/GHOST/commit/f67f26e92dbd110c2edfac06921c58900ad74207))
- script BM25 search and count
  ([b1dd2ae](https://github.com/mrtolkien/GHOST/commit/b1dd2ae431387ba9c6b0ebd66d87ac2e3c9c1aa0))
- script CRUD functions
  ([35b803b](https://github.com/mrtolkien/GHOST/commit/35b803b6b5474f5f918878777b05701e7bef46d8))
- send boot sequence complete DM to operator on Discord ready
  ([515711a](https://github.com/mrtolkien/GHOST/commit/515711aaef624b6b70e779cc3ceb0bc624bd2880))
- shared Chrome sidecar for browser tool and crawl4ai
  ([74f13bf](https://github.com/mrtolkien/GHOST/commit/74f13bf74ac56943303e04d4cb04344e9a7b03df))
- split CLI into reference import (git/crawl) and document import (url/file)
  ([31c17ef](https://github.com/mrtolkien/GHOST/commit/31c17efc0c89cb67ba4f84c9037a5784c4900dad))
- store and load image content blocks in DB
  ([57c2727](https://github.com/mrtolkien/GHOST/commit/57c2727090be85dae2e64a8fa9897cc16de59a15))
- store file_hash on real-time file change events
  ([3e35fb3](https://github.com/mrtolkien/GHOST/commit/3e35fb32f9e4473d7565681ef49ee00903f98c76))
- suppress URL auto-embeds on GHOST response messages
  ([c9ef07e](https://github.com/mrtolkien/GHOST/commit/c9ef07ee0b156116390853d59a359a67ea2d29b6))
- switch workspace flake template to home-manager
  ([b4f9b79](https://github.com/mrtolkien/GHOST/commit/b4f9b79b84847b08368d37582cd057df063276c4))
- two-phase tool call rendering in Discord (send + edit with results)
  ([17f6452](https://github.com/mrtolkien/GHOST/commit/17f6452d3a5c40b92180011f20fd489233af3810))
- update subagent-development skill to reference Lua agents
  ([9dbe50b](https://github.com/mrtolkien/GHOST/commit/9dbe50b4668c4da8cb1703caa22fc3ba023e7b09))
- use crane for incremental nix builds
  ([5659be8](https://github.com/mrtolkien/GHOST/commit/5659be85e630ebc308d788c6d0aa952978d9642e))
- warm up nix flake on daemon boot
  ([98eb3d2](https://github.com/mrtolkien/GHOST/commit/98eb3d28e2d507719d438953846481a2d83454bc))
- watch code/ directory and sync code files to DB
  ([139379b](https://github.com/mrtolkien/GHOST/commit/139379b2d1556e3fe0e3e6a6751a17d77de78265))
- watcher passes archetype from frontmatter to DB
  ([f196fad](https://github.com/mrtolkien/GHOST/commit/f196fadb8b6127e155b487c184a93c57a82fc811))
- wire all onboarding phases together in wizard.rs
  ([dd5b01a](https://github.com/mrtolkien/GHOST/commit/dd5b01a37832ade93293236a99768aa01309e4a2))
- wire confirmation channel through Discord interface
  ([29a58e3](https://github.com/mrtolkien/GHOST/commit/29a58e31feae07d79b47d70f6a08bde5b359fd31))

### Bug Fixes

- actual ping for sqlite
  ([7aebf9c](https://github.com/mrtolkien/GHOST/commit/7aebf9cbc1e7bb6669999cb1f5653e3495bab566))
- add --refresh to nix profile add in ghost update
  ([947ddff](https://github.com/mrtolkien/GHOST/commit/947ddff5c844b4553b263f93c181db9e0f18b6b3))
- add /reboot suggestion on errors
  ([2993712](https://github.com/mrtolkien/GHOST/commit/2993712fb8c5603e8d24e705f2c5f5795f486113))
- add Anthropic variant to Provider config enum
  ([8d73d4a](https://github.com/mrtolkien/GHOST/commit/8d73d4a83d1c600319c6f6a6cd02ae30859fe21f))
- add local bin in nix shll
  ([01df6f5](https://github.com/mrtolkien/GHOST/commit/01df6f53632f072de2ae7d4b3bdf971a243aeee2))
- add nix to PATH in systemd service file
  ([114dc17](https://github.com/mrtolkien/GHOST/commit/114dc17d40b2315e4e4038f8d3073052d565a9d9))
- add observability to discord interactions and reconciliation loop
  ([76cf63e](https://github.com/mrtolkien/GHOST/commit/76cf63eafe3cb83458c68846ea7ae62ad2867739))
- add uv + sqlite to ghost flake
  ([8953bf1](https://github.com/mrtolkien/GHOST/commit/8953bf1689c528ef00cbd9d422b2a4ffc865d8de))
- address CI issues for anthropic provider
  ([3f7e02e](https://github.com/mrtolkien/GHOST/commit/3f7e02e2c755ccf037ef8d92d10285c2a1e73d0b))
- address ci issues from deployment refactor
  ([0db356d](https://github.com/mrtolkien/GHOST/commit/0db356dbeb3ecddbac3566640f9e25c481bf9dbe))
- address code review — session persistence, tracing, SSRF, schema
  ([5e4dc57](https://github.com/mrtolkien/GHOST/commit/5e4dc576499fa1323b05396bc0224ca42b207be8))
- address final review feedback
  ([73baa26](https://github.com/mrtolkien/GHOST/commit/73baa269c73cf6e5ea3bb1fb85916b75f75077f6))
- allow re-reading skills lul
  ([e2e7003](https://github.com/mrtolkien/GHOST/commit/e2e700370a8716be85d95f684c692e7978e4815a))
- allow skipping docs embed for tests
  ([5e3772a](https://github.com/mrtolkien/GHOST/commit/5e3772aff91a330fc91f4599be77496384a77be5))
- Anthropic thinking block ordering + system role handling
  ([6c4c3c9](https://github.com/mrtolkien/GHOST/commit/6c4c3c9735e54b6540fc98d83b01f892bfa7c144))
- better note-taking prompts
  ([a8436b1](https://github.com/mrtolkien/GHOST/commit/a8436b10232195262d59af1298d277705bca4a7c))
- better web searches snippets handling in system prompt
  ([d2a030e](https://github.com/mrtolkien/GHOST/commit/d2a030e7b7f074feb7cd2eb38e057c93c13df916))
- **browser:** clear tabs on reconnect, use BrowserManager in web_fetch
  ([4faf238](https://github.com/mrtolkien/GHOST/commit/4faf23846ef88c76f6cc56c6afa36a83e7ee0c5f))
- **browser:** embed status context in JSON instead of appending text
  ([b175a74](https://github.com/mrtolkien/GHOST/commit/b175a7406accc7c279b482da0d267d2a47c44c4c))
- **browser:** use base CDP URL in discovery, not session-scoped URL
  ([9b32d4f](https://github.com/mrtolkien/GHOST/commit/9b32d4f9b13a2ae1e219d8984b51f1abcc961baa))
- **browser:** use base CDP URL in discovery, not session-scoped URL
  ([bbf4554](https://github.com/mrtolkien/GHOST/commit/bbf4554b9b07fbf4e0f82a03ccea4dfb72dd55b7))
- circuit breaker only triggers on transient errors, not 400 Bad Request
  ([77699c6](https://github.com/mrtolkien/GHOST/commit/77699c602ec2233df7ed1188709f9401fa65244c))
- clippy warning in browser test helper
  ([3653f8e](https://github.com/mrtolkien/GHOST/commit/3653f8efa67a50a38d61a4a06bf91cced0158473))
- Codex reasoning round-trip requires summary field + add Anthropic thinking live test
  ([1b89e86](https://github.com/mrtolkien/GHOST/commit/1b89e864b4b4e5bc8bff4d8bfb8e9d624275940c))
- collapse nested if in file_edit confirmation flow
  ([9d6b195](https://github.com/mrtolkien/GHOST/commit/9d6b1957e4d7574ea8179734515da8cfbf34854d))
- compute and store file_hash in all write paths
  ([0ab5c27](https://github.com/mrtolkien/GHOST/commit/0ab5c2767d6c9a7a39583caffd15cc6aec870c96))
- correct OAuth headers and system prompt format per pi-mono
  ([1bc4c80](https://github.com/mrtolkien/GHOST/commit/1bc4c80658b2d8b36c832848a6496f53cefe8ab4))
- crawl4ai defaults — domcontentloaded, no remove_overlay_elements
  ([8048bd4](https://github.com/mrtolkien/GHOST/commit/8048bd4f752c0fe00c7c1ea90ec73d61d4f928c9))
- Crawl4AI shared browser session — resolve WS URL, host networking
  ([e1a1b3f](https://github.com/mrtolkien/GHOST/commit/e1a1b3facf855b078c7dd7281c56573ddc6e9b0f))
- darwin SDK + nix-cache fail-fast
  ([b4321ce](https://github.com/mrtolkien/GHOST/commit/b4321ced239441afac193c48096544acf1a7b34e))
- delegate post-update reboot to new binary
  ([5ea6766](https://github.com/mrtolkien/GHOST/commit/5ea6766c37adecc52aecca4d7546b0d97588d426))
- **docs:** extract hero effects to Astro component for MDX compat
  ([97c4758](https://github.com/mrtolkien/GHOST/commit/97c4758aebac0bf8e9de665115183023f19dfad0))
- drain pending steer interrupts on EndTurn to prevent message loss
  ([53c7853](https://github.com/mrtolkien/GHOST/commit/53c7853cd1d400060432131b79e61fcb458bb4bf))
- drop topic_name from reference_fts to enable snippet()
  ([6f69f99](https://github.com/mrtolkien/GHOST/commit/6f69f99eddbff6d791a0d41672243d283bbd2ca3))
- enable loginctl linger during ghost init on Linux
  ([04fe620](https://github.com/mrtolkien/GHOST/commit/04fe62096258862b85bcbd588cb898b1122d9033))
- file watcher syncs to DB even when Ollama is unavailable
  ([2d1b581](https://github.com/mrtolkien/GHOST/commit/2d1b581a8ac5481c3341a0c72f632773cc97276b))
- fix live test — spawn_blocking for batch exporter, ClickHouse query
  ([389b761](https://github.com/mrtolkien/GHOST/commit/389b761e29936ab4db62f751d971d3d1f66754dc))
- fix many issues with the coding agent (tools cwd, session id, ...)
  ([e712ddf](https://github.com/mrtolkien/GHOST/commit/e712ddf9b6a974a68e8607762c993641c33e5e1b))
- fix tools calls with missing responses
  ([7503bf1](https://github.com/mrtolkien/GHOST/commit/7503bf1e921b891766966a82a40bc7edc2b0b323))
- full tools wording pass
  ([b003f61](https://github.com/mrtolkien/GHOST/commit/b003f61d9c4022aa8b3d6aaaf0103ecf219204c8))
- gate browser tests behind live-tests-browser feature
  ([66ef59a](https://github.com/mrtolkien/GHOST/commit/66ef59a5702e81de8b722f4df7d787c9d9cc6ad2))
- ghost browsers serve uses random internal port to avoid conflicts
  ([5c7e39b](https://github.com/mrtolkien/GHOST/commit/5c7e39baf39ca9569d405cbdb3cbe5ed9b97af49))
- go back to nix develop, simpler
  ([9032225](https://github.com/mrtolkien/GHOST/commit/903222547c26f75a438fe56c19e424900f9e1af2))
- graceful Discord shutdown via ShardManager
  ([d2454b0](https://github.com/mrtolkien/GHOST/commit/d2454b02ebec33cfdad6ad571cf70c0ae5fec4cd))
- improve reference-import decision flow for PDF/binary URLs
  ([f2231ae](https://github.com/mrtolkien/GHOST/commit/f2231ae301cc29c5073925902d14b800e14e8a8b))
- improve reference-import decision flow for PDF/binary URLs
  ([7f99db0](https://github.com/mrtolkien/GHOST/commit/7f99db0b0a33c529574fbd6ad2da5c97cebc375b))
- keep PATH in nix shell
  ([a6a3ea3](https://github.com/mrtolkien/GHOST/commit/a6a3ea334678c7f67dd80b0093e283064f50bce2))
- load .env from config dir instead of CWD
  ([23e2ed5](https://github.com/mrtolkien/GHOST/commit/23e2ed5702e313e8810839c7f96ec093f9677884))
- longer snippest in knowledge search
  ([e9b30da](https://github.com/mrtolkien/GHOST/commit/e9b30da53576272c55b5cf990b0a931343cad829))
- make --from-source and --version mutually exclusive in ghost update
  ([3b0e581](https://github.com/mrtolkien/GHOST/commit/3b0e581df24f096239f8b649bb4db1bf1809e83a))
- name the flake in the command
  ([494614e](https://github.com/mrtolkien/GHOST/commit/494614e7157401dabe7210af7e787d4c60b74a14))
- nix build + save the PATH
  ([654ded9](https://github.com/mrtolkien/GHOST/commit/654ded9fc6bb83ad14004a5fe210d5f2fbd499b4))
- onboard.py merges into existing config instead of overwriting
  ([f075d29](https://github.com/mrtolkien/GHOST/commit/f075d2961c20e963c9f79f1051338e388badf426))
- only require body/archetype for create/update, not archive
  ([54f50b8](https://github.com/mrtolkien/GHOST/commit/54f50b8e431ff15c38358fdf57351fe7bd3005dc))
- parallel call_tools + omit empty tools array from provider requests
  ([bec9f47](https://github.com/mrtolkien/GHOST/commit/bec9f4739a4aab1afa3c42e0ded94d1634f23fc3))
- prefer embedding chunk snippet in hybrid search results
  ([1aa4624](https://github.com/mrtolkien/GHOST/commit/1aa46246aa5893b478f3c7b64d0e96de14fbdbd9))
- proper cleanup of tool calls with no responses
  ([87bd394](https://github.com/mrtolkien/GHOST/commit/87bd39411c39c7e8c158d7bbd4a160ad9c723e60))
- reboot via systemctl/launchctl, safer update ordering
  ([18d7726](https://github.com/mrtolkien/GHOST/commit/18d7726f79a2a2b2e5ff78560ecbd4884b46b2be))
- reduce skill over-reading with once-per-conversation rule
  ([4bab802](https://github.com/mrtolkien/GHOST/commit/4bab8020b779e0f3263dc9972d9f7171d9f449d8))
- remove extra packages from nix
  ([a668771](https://github.com/mrtolkien/GHOST/commit/a6687710eb9d733f8f43065b93674ae56750aabc))
- remove legacy darwin framework refs from flake
  ([e85b512](https://github.com/mrtolkien/GHOST/commit/e85b512599f559b392be04e8e0ace739af6120b2))
- remove TODO from standard chat
  ([fef9f73](https://github.com/mrtolkien/GHOST/commit/fef9f7337b2d16026324de01f9bde22d8cb516a4))
- remove unwrap() in production, add timeout to CDP discovery
  ([4b03521](https://github.com/mrtolkien/GHOST/commit/4b03521244f514c270f335ae25e0a5a34f251b4d))
- rename OpenAI to ChatGPT subscription, free-text model input, OAuth instructions
  ([c429e26](https://github.com/mrtolkien/GHOST/commit/c429e265ae66e0891a6a8adae7455b2947a84afd))
- resolve {{skill_dir}} in skills so bundled scripts are findable
  ([fd048e2](https://github.com/mrtolkien/GHOST/commit/fd048e28b64d409376043c24c3d5c4ef2fea84f1))
- resolve_ws_url preserves original host, remove rewrite_ws_host
  ([78ebaa2](https://github.com/mrtolkien/GHOST/commit/78ebaa23153c6a90a6636f38b2be6569feb52578))
- revert snippet() for reference_fts (external content incompatible)
  ([950ff88](https://github.com/mrtolkien/GHOST/commit/950ff8867bbad15608a0f239100d3c400d2bc84b))
- reword nix skill to touch more on self update
  ([23a033e](https://github.com/mrtolkien/GHOST/commit/23a033e83aa2eb7e5351eafe1528780d48812442))
- reword skill
  ([10ff4cc](https://github.com/mrtolkien/GHOST/commit/10ff4cc0dbedbd8ccf62785187ec4f59c24c1be0))
- run nix store gc on boot
  ([dc85446](https://github.com/mrtolkien/GHOST/commit/dc8544612c9f6afe55221e4339db67729ecf888e))
- scripting skill, e2e tests, and test boot performance
  ([4cea27f](https://github.com/mrtolkien/GHOST/commit/4cea27f1474e8e192aa6d5d355ec9ea189f48093))
- skill and descriptions rewording
  ([49b54cf](https://github.com/mrtolkien/GHOST/commit/49b54cf8afc82d13c3d2ee28c5984158f5fc1156))
- some prompt engineering
  ([e110629](https://github.com/mrtolkien/GHOST/commit/e110629482950054e5cb3da3eb9dc0305c300620))
- stop GHOST from always searching knowledge for every query
  ([510e006](https://github.com/mrtolkien/GHOST/commit/510e0069972f7f17f02886e0ba62f9a854aaf1a7))
- store file_hash on update path for references and diary
  ([f1c138f](https://github.com/mrtolkien/GHOST/commit/f1c138fd4c86f5b8c693c66b4a03a6d62fc8ba71))
- strip default tool args from Discord display
  ([b7a00c2](https://github.com/mrtolkien/GHOST/commit/b7a00c2b14aef0c53bf8f26b0467b6b33c3c6076))
- suppress chromiumoxide WS deserialization warnings
  ([b4049af](https://github.com/mrtolkien/GHOST/commit/b4049af51633a09502b59b381768deac60883f71))
- symlink for the systemd entry
  ([b6ebdac](https://github.com/mrtolkien/GHOST/commit/b6ebdacba311716edce7fa2e5491666508327727))
- thread channel_id from Discord to shell child processes
  ([bc95dca](https://github.com/mrtolkien/GHOST/commit/bc95dca3d58fadaa3a4407832aa0843f098f7650))
- tons of small fixes (path cwd, tracing CLI, ...e
  ([b06011d](https://github.com/mrtolkien/GHOST/commit/b06011db9d5640eeabee0183c5fd2fa1f439f233))
- truncate_snippet takes chars across lines, not just first line
  ([69c3d8a](https://github.com/mrtolkien/GHOST/commit/69c3d8a770a2313c3935fb06a0db899258a4f7f8))
- update all fetch() callers for chrome_cdp_url parameter
  ([cf1b862](https://github.com/mrtolkien/GHOST/commit/cf1b862052d386722a44b5ea9104cebd7bfbc95d))
- update include_str path for default-flake.nix after move to deploy/
  ([2777cb2](https://github.com/mrtolkien/GHOST/commit/2777cb2631386518e90bff9c924cfa5598af0353))
- UPSERT references + better feedback on import finished
  ([42399e9](https://github.com/mrtolkien/GHOST/commit/42399e9bf9d7ce2dc3288b96b62a88994b273c40))
- use cargo:: double-colon syntax consistently in build.rs
  ([1a7c9a0](https://github.com/mrtolkien/GHOST/commit/1a7c9a068a4e0cf2677d270df7f4fb344d5e150d))
- use floor_char_boundary to prevent panics on multibyte UTF-8 truncation
  ([c8f2d04](https://github.com/mrtolkien/GHOST/commit/c8f2d04043c91bb9051486ee5f040d99e44673e2))
- use from_path_override for .env reload + fix stale comment
  ([4f8fd14](https://github.com/mrtolkien/GHOST/commit/4f8fd14c77f9421e9c8c6ada7e03a5ac68cfaba1))
- use FTS5 snippet() for context-aware search snippets
  ([f499849](https://github.com/mrtolkien/GHOST/commit/f4998499da5ea6779584753692cb925c1103739c))
- use FTS5 snippet() for reference search results
  ([dc8d6fa](https://github.com/mrtolkien/GHOST/commit/dc8d6fabbad372004741f77768c70b0a2746c820))
- use nix profile upgrade for default ghost update
  ([9d3a830](https://github.com/mrtolkien/GHOST/commit/9d3a83089fbc53aca53997b2a4c723ffd847f967))
- use reqwest-blocking-client for OTLP batch exporter compatibility
  ([0927c9e](https://github.com/mrtolkien/GHOST/commit/0927c9e09f7bfeae45729230608c3cf0f6ea50fe))
- use transactional embedding persistence to prevent partial state
  ([a22dacf](https://github.com/mrtolkien/GHOST/commit/a22dacfec12f96c38c0498df748acb5ac8e39626))

### Refactoring

- bundled.rs uses .bundled/ shadow directory with three-way merge
  ([bb0d0ac](https://github.com/mrtolkien/GHOST/commit/bb0d0acc0ea478fb5f7e3493ebe2ea5afb5374f0))
- change Tool::execute return type to ToolOutput
  ([b5e3cc8](https://github.com/mrtolkien/GHOST/commit/b5e3cc8426920bd9428183b05e9238369ef77375))
- change ToolContext.config to Arc&lt;Config&gt;
  ([67834f3](https://github.com/mrtolkien/GHOST/commit/67834f3f96a8273ba1787f438a141fe468d54f99))
- create central bundled file registry
  ([b7fc607](https://github.com/mrtolkien/GHOST/commit/b7fc60763bd7f66b1d01760cebfdb2a622094b0c))
- extract knowledge/diary.rs from files.rs
  ([f03a044](https://github.com/mrtolkien/GHOST/commit/f03a044298c8d30c121c6e1c16705daa146d1ef3))
- extract knowledge/notes.rs from files.rs
  ([1df1719](https://github.com/mrtolkien/GHOST/commit/1df1719cb058b8a2067e99da0119d91626099f7b))
- extract URL matching to shared src/web/url_match.rs
  ([f602d7e](https://github.com/mrtolkien/GHOST/commit/f602d7e5a5f9db92ef8c938c1a5cc6a2105c32c0))
- make watcher process_change pub(crate) for reconciliation
  ([111289c](https://github.com/mrtolkien/GHOST/commit/111289cbb55b190cb7dd14a7d53449e6e813af0d))
- merge reconcile_embeddings into reconcile_filesystem flow
  ([2a05afb](https://github.com/mrtolkien/GHOST/commit/2a05afbd5415a52ef29e33c783347f5eaf686a3f))
- migrate logfire:: macro calls to standard tracing::
  ([f849f82](https://github.com/mrtolkien/GHOST/commit/f849f82bc4829a7d72025b2f11e383943a23de42))
- migrate SessionChat, PromptRenderer, AgentRunner, AgentContext to SharedConfig
  ([4285368](https://github.com/mrtolkien/GHOST/commit/4285368f08b0a548986d650fbf5ff382c19e600b))
- migrate watcher and reconciliation loop to SharedConfig
  ([e7a224f](https://github.com/mrtolkien/GHOST/commit/e7a224f396fb6586f3c98a3b4660c4c9a209c3f6))
- move all service configs to assets/services/, delete deploy/ and templates/
  ([3b09b27](https://github.com/mrtolkien/GHOST/commit/3b09b27e1e3dbf2d8ebc8a7ced371ad05218741a))
- move bundled workspace files to assets/, auto-generate registry via build.rs
  ([4f51586](https://github.com/mrtolkien/GHOST/commit/4f51586c7970417d49c5cbfa50d25619871672d4))
- move docker files to deploy/common/
  ([37ccfdb](https://github.com/mrtolkien/GHOST/commit/37ccfdb3b202e611fc71835057af08402626cc4b))
- move superpowers skills to nested directory with available: coding
  ([8aaa822](https://github.com/mrtolkien/GHOST/commit/8aaa8223fca19b81c28537e8d049eadab2565162))
- remove /usr/local/bin from shell PATH (ghost via nix profile)
  ([8e818bc](https://github.com/mrtolkien/GHOST/commit/8e818bc474540853ac2d7a3f1e756b74c44d2435))
- remove content_hash from embedding table (superseded by file_hash)
  ([dabbf81](https://github.com/mrtolkien/GHOST/commit/dabbf81ac0a456f98a4fd9a9decc9ac8a2e655b3))
- remove dead JobPromptContext code
  ([80b0cc5](https://github.com/mrtolkien/GHOST/commit/80b0cc5b17be722a93f3a946840a0a63d5c4ecc3))
- remove inline embedding from reference imports
  ([4ac8cbc](https://github.com/mrtolkien/GHOST/commit/4ac8cbc334abbe4ce186e256085c3047a29dfa47))
- remove should_trigger from chat-reflection agent
  ([e887fe2](https://github.com/mrtolkien/GHOST/commit/e887fe267bc4b4394bb00526faa5500d388974fd))
- remove should_trigger from scripting layer
  ([36ce613](https://github.com/mrtolkien/GHOST/commit/36ce613b6f63c7e64b8ae715f8e78d4dbfc701bf))
- remove unused legacy install_service_file function
  ([3d05682](https://github.com/mrtolkien/GHOST/commit/3d0568232301cd946b4103e21224d10421ecf75f))
- rename test suites and split feature flags for clarity
  ([38276da](https://github.com/mrtolkien/GHOST/commit/38276daad370698f95209f58485292f7b3157749))
- rename web/browser.rs to web/crawl4ai.rs
  ([7b8b10a](https://github.com/mrtolkien/GHOST/commit/7b8b10a843238c8943851af2cc5b6cd3397c7507))
- reorganize daemon e2e tests into tests/daemon/ folder
  ([655fdac](https://github.com/mrtolkien/GHOST/commit/655fdac978344e44a6f817703199d6b78bb6a34e))
- replace BootResult tuple with DaemonHandle struct
  ([d464e02](https://github.com/mrtolkien/GHOST/commit/d464e02d550c6706f27e368cd3b7591bbda9eb0b))
- replace completion_tx with event_tx in ToolContext and shell
  ([d4c7a45](https://github.com/mrtolkien/GHOST/commit/d4c7a45f7d2b0a49d3b02efd7909ae21678ef8b4))
- replace cross-skill read_file paths with name-only references
  ([3fdb09c](https://github.com/mrtolkien/GHOST/commit/3fdb09cf3987719f156cc2ea8eb9bd4015dc8b7e))
- replace same-skill read_file paths with relative extra references
  ([d9fe0a4](https://github.com/mrtolkien/GHOST/commit/d9fe0a4afc85918b7b6553f5f5e5ced166661352))
- replace watchers with unified session event handler
  ([1140afb](https://github.com/mrtolkien/GHOST/commit/1140afb0cd3d23cdd1c3c976e9cd07d5fb05ed45))
- rewrite idle triggers to be fully DB-driven
  ([d61fbb1](https://github.com/mrtolkien/GHOST/commit/d61fbb1128bf8decd6d123aa463443d2f2ed15a5))
- shell flake provides tools only (ghost via nix profile)
  ([1ff00a2](https://github.com/mrtolkien/GHOST/commit/1ff00a2da54859918072b8910330eb478f6355c3))
- switch embedding client from Ollama to OpenAI-compatible API
  ([a163b0c](https://github.com/mrtolkien/GHOST/commit/a163b0c95962264be35067d3fef9445bb4af0cc5))
- unify bundled file installation through central registry
  ([47ef8bd](https://github.com/mrtolkien/GHOST/commit/47ef8bdca4eeaed8c973266ce1f80af03cdc316e))
- update import_page/import_file to use new docling client
  ([a4b5151](https://github.com/mrtolkien/GHOST/commit/a4b5151343d5deb54570d7693cd5831c4b56c20f))
- use relative paths in image-generation skill shell commands
  ([b03a19d](https://github.com/mrtolkien/GHOST/commit/b03a19dadfded258b5821faedd3020f84de40f14))

### Documentation

- add anthropic to doc
  ([6a44042](https://github.com/mrtolkien/GHOST/commit/6a44042b717ad90dd27cbf8842c99a58ac0d061b))
- add ctx:call_tools() batch API to design and plan
  ([5dba6be](https://github.com/mrtolkien/GHOST/commit/5dba6be9fc3e32bb6bd113af4f982b29b8010e48))
- add design spec for ghost init onboarding wizard
  ([b536f35](https://github.com/mrtolkien/GHOST/commit/b536f35374456dceb994f6b23c4db2ea4bcb8871))
- add design spec for self-hosted OpenTelemetry with SigNoz
  ([86f5f82](https://github.com/mrtolkien/GHOST/commit/86f5f82bcf0089287083eaf46bb2ca28d179ed8b))
- add diary loading design doc
  ([7648e22](https://github.com/mrtolkien/GHOST/commit/7648e22a1920fc9070cb03cb6e3046b1aeeaa474))
- add diary loading implementation plan
  ([e60ba10](https://github.com/mrtolkien/GHOST/commit/e60ba1037d5636744caf86ee19183b034d82c3ef))
- add documentation task to onboarding implementation plan
  ([bd2b427](https://github.com/mrtolkien/GHOST/commit/bd2b427fd5681354e7afc700a703d7ec9256feeb))
- add favicon
  ([087e51d](https://github.com/mrtolkien/GHOST/commit/087e51db29cb1bda44223ab262a568677b7081e1))
- add Ghost in the Shell redesign spec
  ([fc037de](https://github.com/mrtolkien/GHOST/commit/fc037de8c356bf0c68eab06d795e5ef95b8b28b2))
- add implementation plan for Ghost in the Shell redesign
  ([6d5e360](https://github.com/mrtolkien/GHOST/commit/6d5e36000e6cccfddad864ae444a1b2925e6f7a8))
- add implementation plan for ghost init onboarding wizard
  ([7064bad](https://github.com/mrtolkien/GHOST/commit/7064bad1e5a064c60658d4139515ae50636dc6e9))
- add Linux install guide with Cachix binary cache
  ([e8992b2](https://github.com/mrtolkien/GHOST/commit/e8992b24fa63ca547ae23dcaf7f4500fcb9bf9d7))
- add observability/SigNoz to user-facing documentation
  ([b9d7b66](https://github.com/mrtolkien/GHOST/commit/b9d7b66139fadfb8e144a961653903bad354fbd5))
- add onboarding and services pages
  ([61b86b6](https://github.com/mrtolkien/GHOST/commit/61b86b6c09c25bc8987ef677b5546860c6c44452))
- add rule about reviewing temporary fixes before completion
  ([bd71a8e](https://github.com/mrtolkien/GHOST/commit/bd71a8e9c47a3954e8343c5606fa5935cbedebde))
- add scheduled agents section to agent-creator skill
  ([6048975](https://github.com/mrtolkien/GHOST/commit/6048975252228d25ce7f08ca90e8085d421eee5e))
- add token efficieny goal to home page
  ([ddbf857](https://github.com/mrtolkien/GHOST/commit/ddbf857a677ccb4bd83916cbbfeae485d91db25e))
- better index
  ([0b8c777](https://github.com/mrtolkien/GHOST/commit/0b8c7775465d0e5b8e3e79b780ecc2ac0db8cbe4))
- browser tool design spec and implementation plan
  ([0a8bda9](https://github.com/mrtolkien/GHOST/commit/0a8bda9be4eb74103a002f11eaad2c3ca3aff5e7))
- consolidate installation into single page, enhance services with GPU info
  ([388277a](https://github.com/mrtolkien/GHOST/commit/388277a19e630e12141b979d928c82058e7928ef))
- crawl4ai implementation plan — HEAD routing, agent options, live tests
  ([2afa5ab](https://github.com/mrtolkien/GHOST/commit/2afa5ab847a78affb3bcbeb9c9854133f17b9604))
- daemon-level e2e testing design
  ([0d42761](https://github.com/mrtolkien/GHOST/commit/0d4276103cc56f43ec7266d3051c54862787a540))
- daemon-level e2e testing implementation plan
  ([f8e812c](https://github.com/mrtolkien/GHOST/commit/f8e812c52bd9b6f79256e795666a03e5b2e40e68))
- design spec for cron agents, ctx:call_tool, and e2e test
  ([8854710](https://github.com/mrtolkien/GHOST/commit/88547108c5364c02eab151a81b4a41cbf34cf172))
- document document import
  ([db39bf0](https://github.com/mrtolkien/GHOST/commit/db39bf0bc2c23d8abc06c3d59bea330a10845629))
- document ghost config reload command
  ([7cac5cf](https://github.com/mrtolkien/GHOST/commit/7cac5cf2dc845945595f60d4eb6313fb752188bc))
- document linger issue
  ([85da751](https://github.com/mrtolkien/GHOST/commit/85da751a7acb3f206fb953c8a77f9a3010ea945a))
- document reference update command in skill
  ([f230939](https://github.com/mrtolkien/GHOST/commit/f230939b6418f23279a931a2b4e67f7dd336ac68))
- document self-update flow in nix-shell skill
  ([8cd8b59](https://github.com/mrtolkien/GHOST/commit/8cd8b598e04a8d522f9e4518e249190a4a3de63c))
- finalize docs
  ([52e51dc](https://github.com/mrtolkien/GHOST/commit/52e51dcb723987392ff90d3ab61a8090ad6f4529))
- fix docs URLs + specs review
  ([cf84cd6](https://github.com/mrtolkien/GHOST/commit/cf84cd638cb7a4a1175601b7002fe8ddf00b74c5))
- ghost logo
  ([b169a0f](https://github.com/mrtolkien/GHOST/commit/b169a0f498d28f96ad4a47fdc8c357586d400ee2))
- implementation plan for cron agents + ctx:call_tool
  ([254e444](https://github.com/mrtolkien/GHOST/commit/254e444654e0eeb3342a9402c0d6a6d4712f9aad))
- improve browser-use skill operator handoff section
  ([b17e6ab](https://github.com/mrtolkien/GHOST/commit/b17e6ab337418dd53a8b97680b9c6e65b7790e17))
- more specs
  ([807cf6c](https://github.com/mrtolkien/GHOST/commit/807cf6c71f4d25669da1f292b56008b256963c4f))
- nest install pages under Installation group in sidebar
  ([ef9619a](https://github.com/mrtolkien/GHOST/commit/ef9619a58c24409b073afb0618ba051eb2a81e23))
- plans and specs
  ([2ea5118](https://github.com/mrtolkien/GHOST/commit/2ea5118ae41057876a72d50223e73104bb348407))
- plans and specs
  ([9b0bf41](https://github.com/mrtolkien/GHOST/commit/9b0bf41d36b0754482c06ec116690c3c7184ef5d))
- remove favicon background
  ([1d06a92](https://github.com/mrtolkien/GHOST/commit/1d06a923eeb2c92d4ead86cefd1cd926bd50fbe0))
- remove light theme + other minor stuff
  ([d4a310f](https://github.com/mrtolkien/GHOST/commit/d4a310f513397f23ffffba7e43e867120dd22238))
- remove should_trigger from agent documentation
  ([19320c7](https://github.com/mrtolkien/GHOST/commit/19320c7da23c286b554b5fb83f4e113b47897542))
- remove wrong claim (LLMs suck)
  ([63ebfb5](https://github.com/mrtolkien/GHOST/commit/63ebfb55ab6e22ace5189ee3aaf21b06f040c3d0))
- rewrite nix-shell skill for native deployment
  ([a8aea46](https://github.com/mrtolkien/GHOST/commit/a8aea463f0a0b446f831021802f4c30f25c58dca))
- searxng in docs
  ([e3f1c0a](https://github.com/mrtolkien/GHOST/commit/e3f1c0a5fb81855c06fc9af21c9c379da4c5aeb4))
- split installation into per-platform pages (macOS, Linux, source)
  ([7a2dad1](https://github.com/mrtolkien/GHOST/commit/7a2dad1fd3ef08113178fffd6d738986895a3836))
- update "ghost reboot" references to "ghost config reload" where appropriate
  ([fa7a6c6](https://github.com/mrtolkien/GHOST/commit/fa7a6c6f61abeaa0e13deb92a0d6217e58b2db86))
- update crawl4ai spec with empirical param findings
  ([4479ad6](https://github.com/mrtolkien/GHOST/commit/4479ad691b2cba6c8e8569c64b013b8e99969335))
- update cron agents design and plan with ctx:call_tools() batch API
  ([1cb3720](https://github.com/mrtolkien/GHOST/commit/1cb3720e62d3c66ee679a952a795cbb10e14a886))
- update installation guide for macOS one-line install
  ([de8c31d](https://github.com/mrtolkien/GHOST/commit/de8c31d0efb82d4251cd00585f030a8e8d4615b0))
- update nix-shell skill for home-manager workflow
  ([0b1c2d6](https://github.com/mrtolkien/GHOST/commit/0b1c2d64b362c3bb1ab43c9fdfbb737ef56be471))
- update skills and config for logfire -&gt; opentelemetry migration
  ([c6bbce4](https://github.com/mrtolkien/GHOST/commit/c6bbce4fab036f6483ce71fce04532793ab4d144))

### Tests

- add 6 new live tests + improve tool description
  ([fe6dadb](https://github.com/mrtolkien/GHOST/commit/fe6dadbb61e45772b4b242fad628ba3db4f9724d))
- add code file CRUD, search, walk, and slug extraction tests
  ([af025f3](https://github.com/mrtolkien/GHOST/commit/af025f3fa4b42206fc72bd22844f382ee949379c))
- add first daemon-level e2e test (ark nova import)
  ([8d9ee8d](https://github.com/mrtolkien/GHOST/commit/8d9ee8d8dffab5cee8011d12abf422f25bb081c8))
- add legacy Codex reasoning DB format backward-compat test
  ([3679b45](https://github.com/mrtolkien/GHOST/commit/3679b45a91deb980cb4cd7d13fa251ce13f10f9e))
- add live test for OTLP export to SigNoz
  ([7ed6dd3](https://github.com/mrtolkien/GHOST/commit/7ed6dd350ba93d2a5a8df03b5405edf53eaa3ae1))
- add live tests for image round-trip on both providers
  ([6a62dc4](https://github.com/mrtolkien/GHOST/commit/6a62dc4d33553b2fd703356209c08efdec6fdc39))
- add reconciliation hash-skip and missing-embedding tests
  ([3e87b8e](https://github.com/mrtolkien/GHOST/commit/3e87b8e7de1ce5d8c4ac0f7ed283f19b5ca84639))
- **browser:** add multi-tab and multi-browser integration tests
  ([ae9603b](https://github.com/mrtolkien/GHOST/commit/ae9603b17a59516aa27e5117b86dc52e2995b3f9))
- config loading with anthropic provider
  ([9d6ed3b](https://github.com/mrtolkien/GHOST/commit/9d6ed3b4004beaec666f0a4eac0d30f737bf4d28))
- e2e test for cron agent creation from chat
  ([330ddfb](https://github.com/mrtolkien/GHOST/commit/330ddfbd3fda528428c8c3f92ff537d3cd8d9584))
- full browser tool integration test using blog.tolki.dev
  ([7cc1b63](https://github.com/mrtolkien/GHOST/commit/7cc1b638b8455268c7f8cbde6cd766782fcf9a33))
- live test for reference update with diff and orphan protection
  ([86d2566](https://github.com/mrtolkien/GHOST/commit/86d25669b4127ef570b0444d5a549ace04dc47c2))
- live test with full Ghost toolset against Anthropic OAuth
  ([59d2797](https://github.com/mrtolkien/GHOST/commit/59d2797ad037ac65c6cf3cb9ee089d207a09cffb))
- live tests for Anthropic OAuth provider
  ([8c8bbc0](https://github.com/mrtolkien/GHOST/commit/8c8bbc00f20dcb9b0ba05ee12bba590bbf55d444))
- parser roundtrip tests for archetype and new frontmatter fields
  ([82905be](https://github.com/mrtolkien/GHOST/commit/82905bea67352f296b9631dbf282e83991461250))
- session concurrency guard and EndTurn interrupt drain
  ([de8ccae](https://github.com/mrtolkien/GHOST/commit/de8ccaef46a67d29b8d23ec432c0ff215fc59f0a))
- unit tests for ctx:call_tool and ctx:call_tools in build hook
  ([bea78b4](https://github.com/mrtolkien/GHOST/commit/bea78b4a48b6bc7d60e714b46f02d2a57884ea8e))
- update live tests + add crawl4ai primary path tests
  ([3d00163](https://github.com/mrtolkien/GHOST/commit/3d001634e93f5d22919d9c39de98538b6c97715f))
- update live tests for ContentBlock::Thinking variant
  ([e407aca](https://github.com/mrtolkien/GHOST/commit/e407acab28219bb70acad04ce1b8ecf6bfa737f2))
- update test helpers for session event bus
  ([dd64cef](https://github.com/mrtolkien/GHOST/commit/dd64cefdb42e0813348845877b20ec08b7c15a1b))
- update tests for archetype required param, format + schema regenerate
  ([79249fb](https://github.com/mrtolkien/GHOST/commit/79249fbaf3eaaf62365b3a93895b352784a6a2e8))
