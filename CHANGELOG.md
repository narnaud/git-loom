# Changelog

## [0.13.0](https://github.com/narnaud/git-loom/compare/v0.12.0...v0.13.0) (2026-03-07)


### Features ✨

* add `show` command for viewing commits by short ID ([#36](https://github.com/narnaud/git-loom/issues/36)) ([fdf954a](https://github.com/narnaud/git-loom/commit/fdf954a7883050a208d57284b8adcba009f62085))
* **branch,reword:** warn when branch name matches hidden pattern ([54e0b2c](https://github.com/narnaud/git-loom/commit/54e0b2c1ae8dfa28b7b4b05912f6449d1334ddab))
* **drop:** support dropping files and all local changes ([5c2727d](https://github.com/narnaud/git-loom/commit/5c2727d45efa523bbe58a86a9becfd25d516b576)), closes [#50](https://github.com/narnaud/git-loom/issues/50)
* **fold:** add --create flag to create and move commits into new branches ([ba6ef5a](https://github.com/narnaud/git-loom/commit/ba6ef5ab1a328c2550ffa4f81209a83533a60a7a)), closes [#37](https://github.com/narnaud/git-loom/issues/37)
* **push:** add AzureDevOps remote type with az CLI integration ([71dc11d](https://github.com/narnaud/git-loom/commit/71dc11d79f8f989abde1ffa9d81e2e5947017731))
* show new commit hash after rewrite operations ([23904c5](https://github.com/narnaud/git-loom/commit/23904c5d738c9c43965b8cd66c7b852bac71aa63)), closes [#19](https://github.com/narnaud/git-loom/issues/19) [#40](https://github.com/narnaud/git-loom/issues/40)
* **status:** group tracked changes before untracked files ([b95e117](https://github.com/narnaud/git-loom/commit/b95e117dc090d889ee111cf0601c17d7d45574cf))
* **status:** hide local-* branches by default with --all flag ([8664598](https://github.com/narnaud/git-loom/commit/866459890cf856f08832b39fd2b1080f30a18700)), closes [#38](https://github.com/narnaud/git-loom/issues/38)
* **status:** multi-column layout for untracked files ([0ebc6fa](https://github.com/narnaud/git-loom/commit/0ebc6faafd4a4829b78fcf29e4e685936add9d07)), closes [#41](https://github.com/narnaud/git-loom/issues/41)
* **theme:** add --theme flag with light/dark/auto support ([edb251c](https://github.com/narnaud/git-loom/commit/edb251c041188c3205635d8c426540cd19e54ac0))


### Bug Fixes 🐞

* eliminate TOCTOU race in stage_path ([a9fc62e](https://github.com/narnaud/git-loom/commit/a9fc62e108b385c10d8979331567c400e4500e90))
* **reword:** show reworded commit hash instead of merge commit hash ([c435705](https://github.com/narnaud/git-loom/commit/c4357058fd3ac55236549cf9f0764300c7ca66dd)), closes [#35](https://github.com/narnaud/git-loom/issues/35)
* **shortid:** enforce 2-char minimum and commit-first allocation ([5fb0062](https://github.com/narnaud/git-loom/commit/5fb0062e5e8d22f75944381091086411e2305ea6))
* **status:** New file should appear with red ?? ([0fbbdc5](https://github.com/narnaud/git-loom/commit/0fbbdc5ec99f20c5784e0ef18da808f1ca3be50b))


### Documentation

* add show command to README, CLAUDE.md, and docs site ([af6c417](https://github.com/narnaud/git-loom/commit/af6c417b91f16aa2ef900a5afd8bbf4025d8d3bd))
* **drop:** document file drop and zz drop-all behavior ([0ede693](https://github.com/narnaud/git-loom/commit/0ede69358d5ce9ff6fe601b6b947b9dda4ff5410))
* fix code to avoid scrollbar ([2d4f5cd](https://github.com/narnaud/git-loom/commit/2d4f5cde6f3446d201d75826c018070fc5d05141))
* **fold:** document --create flag ([b582ffe](https://github.com/narnaud/git-loom/commit/b582ffe91887178f37a927b923dc33e8489e20fa))
* **push:** document Azure DevOps support in README and user docs ([d0573ef](https://github.com/narnaud/git-loom/commit/d0573ef05f4ec7d32f231dc9f630729b169e003c))
* **status,branch,reword:** document hidden branches feature ([240548f](https://github.com/narnaud/git-loom/commit/240548f7effadeeb751b0ebffd3f0ff5daebf03f))
* **status:** Update the documentation with latest changes ([0c04d6c](https://github.com/narnaud/git-loom/commit/0c04d6c966b75046738b56ccfa370ff6c8fcd295))
* **theme:** document --theme flag in README and docs ([b98a90a](https://github.com/narnaud/git-loom/commit/b98a90a31cdcc6ac99d56b5a8d869e5ad27d940d))
* update usage instructions for the no command case ([5e9a3e9](https://github.com/narnaud/git-loom/commit/5e9a3e963bf2bcd21d7a69388368b2486b2f889e))


### Changes

* extract shared run_git_captured helper ([69dd5c4](https://github.com/narnaud/git-loom/commit/69dd5c4d6577d15f4d7b55c9b594f803a56ae408))
* reuse run_git_interactive in git_commit ([9f9532b](https://github.com/narnaud/git-loom/commit/9f9532bbdde78c56fc9d9e8d457c1e91e289fc23))
* **show:** pass branch name directly to git show ([5dbdbfb](https://github.com/narnaud/git-loom/commit/5dbdbfbc5f38339d1d881f6ac6e1fa1f81aea531))
* **tests:** extract common git ops into TestRepo helpers ([65af6cd](https://github.com/narnaud/git-loom/commit/65af6cd56d050d38748d1ad3e1f817166db53064))


### Other

* add show command to completion scripts ([cd7f32a](https://github.com/narnaud/git-loom/commit/cd7f32a88e821240759e88cda40836de6dd309d3))
* **claude:** Update permissions ([1c84efe](https://github.com/narnaud/git-loom/commit/1c84efea3314e048948fd65fa35bbc2f6f479bd5))

## [0.12.0](https://github.com/narnaud/git-loom/compare/v0.11.0...v0.12.0) (2026-03-06)


### Features ✨

* **commit:** allow loose commits even when local commits exist ([c5ee426](https://github.com/narnaud/git-loom/commit/c5ee426f99da22016169c6b8d74aa411283477ae))
* support `fold zz <commit>` to fold all working tree changes ([f0b76ba](https://github.com/narnaud/git-loom/commit/f0b76ba25a1a004139a82f81f55e265184bfa4dd))
* **trace:** add per-invocation command audit trail ([667b13e](https://github.com/narnaud/git-loom/commit/667b13eb5bace89905592e5971b638f788f84f3f))


### Bug Fixes 🐞

* **branch:** Fix creation of branch on a commit which resolves to HEAD ([6c7e8d9](https://github.com/narnaud/git-loom/commit/6c7e8d933a3cdcc59ccfae00456dabd3d55bb506))
* detect upstream remote for GitHub PR target repo ([6a0f19e](https://github.com/narnaud/git-loom/commit/6a0f19eb76ce10cde5d373440e42cb9b5bc8b704))
* don't recurse into untracked dirs (too noisy) ([bdb3cf5](https://github.com/narnaud/git-loom/commit/bdb3cf5ced97a17d3cd51bae2d88af71d376ec87))
* fold commit to branch without section in Weave graph ([b043cb2](https://github.com/narnaud/git-loom/commit/b043cb2c08e1483c64f4b69dd275cdad6fa30e7e))
* hide upstream's local counterpart from status when at merge-base ([0715f7d](https://github.com/narnaud/git-loom/commit/0715f7ddd2b5813d4ccf440933a9877dfb13785b))
* not giving a new name for a branch in git reword is a noop ([be3ae12](https://github.com/narnaud/git-loom/commit/be3ae120aa2ab199c61f8dfdb179b6127cae0d20))
* only create loose commits when branch name matches upstream ([942cf85](https://github.com/narnaud/git-loom/commit/942cf8599c996ec671a365b2b2696b6039ab29e7))
* **push:** Fix re-pushing a branch to github ([408e4c2](https://github.com/narnaud/git-loom/commit/408e4c20eecc4f86bd86ddc3188c3b7073d4f1ef))
* **trace:** Don't show '\' on Linux. Show '/' on Windows instead ;) ([2bb0a26](https://github.com/narnaud/git-loom/commit/2bb0a2697edba90676989cdba912a87b77d3126e))
* **weave:** add # comment marker before commit messages in todo ([7f9c8e1](https://github.com/narnaud/git-loom/commit/7f9c8e1160668c9a23e0c37b58466c5df2317012))


### Performance Improvements ⚡

* skip file gathering in resolve_shortid when not needed ([028c1f5](https://github.com/narnaud/git-loom/commit/028c1f5e90a1815b4d689cde625c5d5057fc61f3))


### Documentation

* add trace command documentation ([48cb8f3](https://github.com/narnaud/git-loom/commit/48cb8f310b384a1bd5ac08d9919a33b490640dfe))
* Fix pre-commit command in README ([c874c5f](https://github.com/narnaud/git-loom/commit/c874c5f4d401a986d331ec76b2a811adedec118a))
* reorder commands in documentation ([0bfaa7b](https://github.com/narnaud/git-loom/commit/0bfaa7bb138db1ad64d180021f361081af3bc3cc))
* update all specifications and documentation based on last changes ([74b154e](https://github.com/narnaud/git-loom/commit/74b154e5f5c3af2d416451d68ea931a7a65855ae))


### Changes

* add Target::expect_branch() and use it in push and commit ([4b7fdc8](https://github.com/narnaud/git-loom/commit/4b7fdc8b53dcd54d2a5b28b96873ae3e05c47674))
* avoid double path construction in detect_remote_type ([41518ee](https://github.com/narnaud/git-loom/commit/41518ee3f60bd105e89081ef699e39f4cfce75d9))
* compute find_owned_commits once in drop_branch_with_info ([403e4a8](https://github.com/narnaud/git-loom/commit/403e4a8cb1575b0a7196f2df17c1229cc918c8f7))
* extract pending_refs helpers in weave::to_todo ([090533f](https://github.com/narnaud/git-loom/commit/090533fae6c4a47a2529bb0e46eed491c367c061))
* remove duplicate do_split_at_pause in split.rs ([d675669](https://github.com/narnaud/git-loom/commit/d6756699ce493135ca081c4c2f79ce0138434be0))
* reuse git::upstream_local_branch in extract_target_branch ([112a21b](https://github.com/narnaud/git-loom/commit/112a21bc4022d7e96c87665cdb0becb1e2d8c91f))
* Update the help to organize commands ([66147ec](https://github.com/narnaud/git-loom/commit/66147ecdddb27f219000904f19e85f706005339c))
* use e.to_string() instead of format!("{}", e) ([777ce87](https://github.com/narnaud/git-loom/commit/777ce871ef2cfa90ea4fcbd630e5d7dc0dbd6a6c))


### Tests

* Fix tests on Linux ([08ae7f7](https://github.com/narnaud/git-loom/commit/08ae7f79621f4bf653f414ff713a877c17887b35))


### Other

* Add split command to shell completions ([a0bcbcf](https://github.com/narnaud/git-loom/commit/a0bcbcf46435e5a5035c0bd45ab9c3255d24a357))
* **deps:** bump actions/checkout from 4 to 6 ([0990c22](https://github.com/narnaud/git-loom/commit/0990c22d2d1979abe37ff6816d7326e8234ee597))
* **deps:** bump actions/upload-pages-artifact from 3 to 4 ([c5db278](https://github.com/narnaud/git-loom/commit/c5db278f9b6e630e00fc138299db6e7e2045f940))
* **deps:** bump chrono from 0.4.43 to 0.4.44 ([124b448](https://github.com/narnaud/git-loom/commit/124b448d14b377f84e84a86a681b23626e58806d))
* **deps:** bump clap from 4.5.57 to 4.5.60 ([3a7ced9](https://github.com/narnaud/git-loom/commit/3a7ced9061a74504083264ca10bd1594819d6bca))
* **deps:** bump tempfile from 3.24.0 to 3.26.0 ([dbfc4a4](https://github.com/narnaud/git-loom/commit/dbfc4a48e3eff6c9336d2f681e203300d1f071fe))
* **deps:** Update all dependencies ([0156d99](https://github.com/narnaud/git-loom/commit/0156d99f9ae20d48f40e1d5ed7f01b564c07282e))
* fix clippy error (if-let chain) ([f7f0baa](https://github.com/narnaud/git-loom/commit/f7f0baa408e81537cc2e594baebc71ae11a9a3e3))

## [0.11.0](https://github.com/narnaud/git-loom/compare/v0.10.0...v0.11.0) (2026-03-01)


### Features ✨

* Add context commits before base in `loom status` ([aa9bb99](https://github.com/narnaud/git-loom/commit/aa9bb997d275f5099233ff681321cb59365f91da))
* Add split command to split a commit into two ([98b8ce6](https://github.com/narnaud/git-loom/commit/98b8ce6a99bea752ff291ea3bcea7105392e67e3))
* Auto-create loose commit when integration branch matches remote ([8274135](https://github.com/narnaud/git-loom/commit/82741353ae0a75e284c98d35d5837ca37378de27))
* Drop on a file restores it with confirmation ([a07697d](https://github.com/narnaud/git-loom/commit/a07697d4b6bdcbaaef7d8247ff08898ef6164bd7))


### Bug Fixes 🐞

* Preserve woven merge topology during update rebase ([5af65cf](https://github.com/narnaud/git-loom/commit/5af65cfadec7894d805e84b8e13beda9eeb49b18))
* Skip PR creation when pushing the upstream branch on GitHub ([0a11b3c](https://github.com/narnaud/git-loom/commit/0a11b3cb8c94090c24559b12dac63ed6947b23bb))
* Use git editor instead of inquire prompt for split commit message ([3d0dc19](https://github.com/narnaud/git-loom/commit/3d0dc198f4f5e437611cf9cd3eee2be0300be8d1))


### Documentation

* Add absorb command and missing drop --yes flag ([4b5f0e7](https://github.com/narnaud/git-loom/commit/4b5f0e7af4fa329837e715439972b40fdcf44e54))
* Add context commits to status spec and documentation ([d76301d](https://github.com/narnaud/git-loom/commit/d76301d36043234ec8d1f6ec8bca2114a3a315cb))
* Add split command and loose commit documentation ([484ff6b](https://github.com/narnaud/git-loom/commit/484ff6b1e525b33c622f0c5b94d0a6c9e0a3a2a6))
* Update drop specs and docs ([5546494](https://github.com/narnaud/git-loom/commit/55464940aaf97d383defe3d88e4501877fe3916c))


### Other

* Add CODEOWNERS ([148978f](https://github.com/narnaud/git-loom/commit/148978f88ff8730288be5f2f3e0bf94c833b453e))
* Add release profile for optimized binary size ([d6c10af](https://github.com/narnaud/git-loom/commit/d6c10af7f85cc2accd1b29d93b765edb92b17422))
* Fix pre-commit codespell skip file ([7fbf725](https://github.com/narnaud/git-loom/commit/7fbf725be4a166ae00395cb6d4dd2cd3d94bc662))
* Fix spelling ([e25a2d7](https://github.com/narnaud/git-loom/commit/e25a2d745aee7e4fce4854a1bdc5bfccbd8abe10))
* Update pre-commit hooos ([0afe346](https://github.com/narnaud/git-loom/commit/0afe3467d42145d139879f87d763cde6add9da0e))

## [0.10.0](https://github.com/narnaud/git-loom/compare/v0.9.1...v0.10.0) (2026-02-28)


### Features ✨

* Add absorb command to auto-distribute changes into originating commits ([ec44642](https://github.com/narnaud/git-loom/commit/ec44642f5638f8bd695f757b3b959d7f88c2bb06))


### Bug Fixes 🐞

* **ci:** Vendor OpenSSL for aarch64-linux cross-compilation ([39624d4](https://github.com/narnaud/git-loom/commit/39624d41aa78b561bb0fd07fc22ed4d0a4015aae))


### Performance Improvements ⚡

* Avoid multiple calls to gather_repo_info ([4a86dfb](https://github.com/narnaud/git-loom/commit/4a86dfbe31c5cd5c6881a8c64135590d434ff468))


### Documentation

* Add screencast to README and documentation introduction ([a2c9806](https://github.com/narnaud/git-loom/commit/a2c9806cd2884ae1e8e4abe82a7074bcf977715c))


### Other

* **ci:** Remove x86_64 macOS target and suppress brew warning ([d9c85e8](https://github.com/narnaud/git-loom/commit/d9c85e89901cd91daf88fd29c5273942c5f8fea3))
* Update shell completions for absorb, drop --yes, and status --files ([2165feb](https://github.com/narnaud/git-loom/commit/2165feb0aac21aca740df4b6569cdfd0d491bf85))

## [0.9.1](https://github.com/narnaud/git-loom/compare/v0.9.0...v0.9.1) (2026-02-27)


### Bug Fixes 🐞

* Add missing version flag ([2bc087c](https://github.com/narnaud/git-loom/commit/2bc087c48d9846ffbd576edb8b8967c8c7f52d8b))
* **ci:** Fix build on Linux and Mac ([7483944](https://github.com/narnaud/git-loom/commit/7483944d20b57e67e8e61c8ec1ad5138401ce77b))
* **cli:** Add --files bare option, to match status ([5eba86a](https://github.com/narnaud/git-loom/commit/5eba86a6f6167b34e6dfa98e3cdbc851cdf213a3))
* **commit:** Preserve working-tree changes on rebase conflict rollback ([db02745](https://github.com/narnaud/git-loom/commit/db027456a5c1ec301e1c94b36a155d31fd7ee49c))
* **drop:** Add confirmation prompt and preserve inner branches when dropping stacked branches ([1a08451](https://github.com/narnaud/git-loom/commit/1a0845102cca567cb6793b875c58f59a7399cd7f))
* Fail early in bare repository ([b110efd](https://github.com/narnaud/git-loom/commit/b110efdec3f46c0759a5bfc03492a5acd248a1ab))
* **fold:** Prevent autostash conflicts when folding files into woven branch commits ([818ac4a](https://github.com/narnaud/git-loom/commit/818ac4af0e3d8e30acd655ead8c2ac1a24ebc618))
* Strip misleading hint lines from aborted rebase errors ([0e44089](https://github.com/narnaud/git-loom/commit/0e4408906d54610b5b4831fed08960a830a5ada0))
* **update:** Show clean error on rebase conflict instead of raw git hints ([c0e0fb4](https://github.com/narnaud/git-loom/commit/c0e0fb4aa53f2679bb294a06e7dedb4d8005b091))
* **weave:** Update merge commit message when commit moves between branches ([56042c8](https://github.com/narnaud/git-loom/commit/56042c8422e92c7bab15bb8346b7af7b7f80168c))


### Other

* **ci:** Add dependabot ([2a49ec1](https://github.com/narnaud/git-loom/commit/2a49ec192c1d6eef5d2a8ba7f8f4a4ccee28991d))
* Update inquire ([7fcb9cc](https://github.com/narnaud/git-loom/commit/7fcb9ccf278a668bed703dc65ac0660f46c5c1bd))

## [0.9.0](https://github.com/narnaud/git-loom/compare/v0.8.0...v0.9.0) (2026-02-26)


### Features ✨

* **ci:** Publish to crates.io and package on Linux and Mac ([8cbe96e](https://github.com/narnaud/git-loom/commit/8cbe96ece2db6b07135535a6c9f99eb7bdc02586))
* **update:** Show latest upstream commit after successful update ([a607fd1](https://github.com/narnaud/git-loom/commit/a607fd1de23ad9098ecd17bab1158809a49ef592))


### Documentation

* Update installation steps ([f4a1fe9](https://github.com/narnaud/git-loom/commit/f4a1fe94f537a204ef6f5df27ed5fb13a0bd3d3b))


### Other

* Add crates.io package metadata ([938a2bc](https://github.com/narnaud/git-loom/commit/938a2bcd1625c1e3b7c5531e2998af0b70f9f81a))

## [0.8.0](https://github.com/narnaud/git-loom/compare/v0.7.0...v0.8.0) (2026-02-26)


### Features ✨

* Add colored success/error messages ([ab37051](https://github.com/narnaud/git-loom/commit/ab37051f467c2cb0cdba00b70124159317b276bc))
* Highlight command-line values in msg output ([73dbe8d](https://github.com/narnaud/git-loom/commit/73dbe8d2fb62a89c8a946fb2bb404e8331720cab))
* Show hint lines with blue arrow on multi-line errors ([8233634](https://github.com/narnaud/git-loom/commit/82336348ca2e93e8cb8d5c5eb29921b5832f160b))
* Support GitHub fork workflow in init and push ([ae9c702](https://github.com/narnaud/git-loom/commit/ae9c702a784ad2ce714a5465fa41590bd41c0251))


### Documentation

* Add mdBook documentation site with GitHub Pages deployment ([c813357](https://github.com/narnaud/git-loom/commit/c813357840db8008613316548ddef4a796d3e564))


### Changes

* Replace cliclack spinner with custom spinner in msg module ([e2aefea](https://github.com/narnaud/git-loom/commit/e2aefeadf57a56b16dee75be5d21664ec40d12d5))
* Replace cliclack with inquire, add prompt helpers in msg module ([3889b21](https://github.com/narnaud/git-loom/commit/3889b211765682a78b5062c2062f45f49e24c833))


### Other

* **claude:** Update permissions ([b95bdd1](https://github.com/narnaud/git-loom/commit/b95bdd190ce58997faad8e2fa9edeec0f1cf4343))

## [0.7.0](https://github.com/narnaud/git-loom/compare/v0.6.0...v0.7.0) (2026-02-23)


### Features ✨

* Add `-f` flag to `git loom status` to show per-commit files ([e4d8a85](https://github.com/narnaud/git-loom/commit/e4d8a8532c4fe393f35d3e1a44b787c68dd1f6df))
* Add CommitFile fold operations to move/uncommit individual files ([8905bf3](https://github.com/narnaud/git-loom/commit/8905bf3dbdd2ff81e81bb1ccc726dd9fce225879))


### Bug Fixes 🐞

* Add rollback and error recovery to rebase-based operations ([bdbf530](https://github.com/narnaud/git-loom/commit/bdbf53049aeb2580e63c1070e9fa25091f1f46a3))


### Changes

* Change default init branch name from "loom" to "integration" ([deba54d](https://github.com/narnaud/git-loom/commit/deba54d5cd4b8c1c8cbea8998bed372826b6d93a))


### Other

* **claude:** Allow cd ([6717b66](https://github.com/narnaud/git-loom/commit/6717b666ac7b888c9d3b492fe99ee8a0dd17c7c7))

## [0.6.0](https://github.com/narnaud/git-loom/compare/v0.5.1...v0.6.0) (2026-02-22)


### Features ✨

* Add `git loom push` to push feature branches to remote ([5a3af25](https://github.com/narnaud/git-loom/commit/5a3af25ab5fe9aec058a542bb3b86523ba77b60e))
* Add uncommit via `git loom fold <commit> zz` ([31fe70a](https://github.com/narnaud/git-loom/commit/31fe70a3c7393ee9e53670e8718bca072052e971))


### Documentation

* Add Configuration section to README ([6e323dc](https://github.com/narnaud/git-loom/commit/6e323dc0c0f4b04b7a9ec2d3b02f75663a425545))


### Changes

* Migrate error handling from Box&lt;dyn Error&gt; to anyhow ([7ccf6c6](https://github.com/narnaud/git-loom/commit/7ccf6c63f0bf40360d68905801fbe14ccfcb2b99))

## [0.5.1](https://github.com/narnaud/git-loom/compare/v0.5.0...v0.5.1) (2026-02-21)


### Bug Fixes 🐞

* Remove completions from completions matcher ([c11909b](https://github.com/narnaud/git-loom/commit/c11909b9693ecc4ef52268395e64c139e931c3f2))

## [0.5.0](https://github.com/narnaud/git-loom/compare/v0.4.1...v0.5.0) (2026-02-21)


### Features ✨

* Add completions for clink and powershell ([8c96ed1](https://github.com/narnaud/git-loom/commit/8c96ed17a95b2b8a6ff00e50e00cf2b96d33088a))


### Documentation

* Remove architecture details from the specifications ([75ba39f](https://github.com/narnaud/git-loom/commit/75ba39f33ccc9f3123d6ece4fde2397efbc9192d))
* Update README.md file ([bc4664c](https://github.com/narnaud/git-loom/commit/bc4664c385e5f3ce3f0eef81e9d47d05eeea5898))


### Other

* Add lua script for clink completions ([27f4ca3](https://github.com/narnaud/git-loom/commit/27f4ca33f333cfb9ad85d7d16895131ef6c6a59e))

## [0.4.1](https://github.com/narnaud/git-loom/compare/v0.4.0...v0.4.1) (2026-02-20)


### Documentation

* Update specs following Weave graph model refactor ([35f47c9](https://github.com/narnaud/git-loom/commit/35f47c9132c0ac9d5286348c469c2d001b4da7ed))


### Changes

* Unified interactive rebase via Weave graph model ([9f2ba3d](https://github.com/narnaud/git-loom/commit/9f2ba3d38342cb9ddc124cb8e602d27b40f1021e))


### Other

* **claude:** Update following weave refactoring ([ead99d7](https://github.com/narnaud/git-loom/commit/ead99d7856cfbca74fe22a9c8fa966ae42dfda3e))

## [0.4.0](https://github.com/narnaud/git-loom/compare/v0.3.0...v0.4.0) (2026-02-19)


### Features ✨

* Add minimum git version check ([f00e75c](https://github.com/narnaud/git-loom/commit/f00e75c85f51c6a8699946f8eae31b79c32f9cc7))
* Allow dirty working tree for some commands ([6b01029](https://github.com/narnaud/git-loom/commit/6b01029ae28dee8aef4e018140b12b603cd9b856))
* **branch:** "Weave" branch if needed ([f215b12](https://github.com/narnaud/git-loom/commit/f215b12a1c7efb94289b8ac5876780142005e889))
* **branch:** Create a new branch with the `branch` command ([11be764](https://github.com/narnaud/git-loom/commit/11be7645afa23b686949ec897f1ae09f62674c35))
* **commit:** Add the commit command ([79b1358](https://github.com/narnaud/git-loom/commit/79b135852dae77a555ff8f6d7b358f079716a88d))
* **drop:** Add the drop command ([ae3d8e9](https://github.com/narnaud/git-loom/commit/ae3d8e9d938b0d8b8f8a6f08c6685ed0fcc2a86e))
* **fold:** Add fold command ([bed1219](https://github.com/narnaud/git-loom/commit/bed1219ea6b0d72b4428eef33b58b786fc54cc91))
* **init:** Add init command ([b37466a](https://github.com/narnaud/git-loom/commit/b37466a85a8aab12a0387e1199b5da8cce4701a9))
* **status:** Use different characters for staged/unstaged status ([c5107e6](https://github.com/narnaud/git-loom/commit/c5107e6c337df4ab241d77614ee0417c8fc4f4dc))
* **update:** Add update command ([892322e](https://github.com/narnaud/git-loom/commit/892322e28c90e91d0c99af9a37805ba55d7cb07c))


### Bug Fixes 🐞

* **commit:** Create parallel branches if the branch has no commits ([fcd8e8f](https://github.com/narnaud/git-loom/commit/fcd8e8ff2ab05e5e1948de9939385b2bb13f4c0e))
* Fix clippy warning, remove unused method ([e3f3aae](https://github.com/narnaud/git-loom/commit/e3f3aae46fecc1b30c2af81644083e38693b7ce3))
* Fix dropping co-located branches ([d424ef9](https://github.com/narnaud/git-loom/commit/d424ef921a13a585c9867ac0f73472b3ea7babd8))
* Moving a commit during the interactive rebase ([e7eff18](https://github.com/narnaud/git-loom/commit/e7eff186e0928d7f59d31678804488a6d2e7802d))


### Documentation

* Add commit command ([2a1c649](https://github.com/narnaud/git-loom/commit/2a1c64981f8e4a92c3a3f398e62cdcc7d2a0750a))
* Add fold command ([ba1226f](https://github.com/narnaud/git-loom/commit/ba1226fbc27c3350b0d77a528a0cf34bb195055b))
* Add init command ([c1814e3](https://github.com/narnaud/git-loom/commit/c1814e33fd933886645dea3affc8cb69c293a95e))
* Add the drop command ([c13bca2](https://github.com/narnaud/git-loom/commit/c13bca27541243cb83a96e084e67492040631aa6))
* Add update command ([f751439](https://github.com/narnaud/git-loom/commit/f7514390242f19b0c285470ec6625409c0f9b363))
* **branch:** Add branch command specification ([cf93149](https://github.com/narnaud/git-loom/commit/cf931491d74ea662e4d11811a213c58e0b444e6b))
* Update documentation to match code ([8a89878](https://github.com/narnaud/git-loom/commit/8a898785ade47bbb1bba0b8b77692036ba5bfe3d))
* Update specs following latest changes ([6356138](https://github.com/narnaud/git-loom/commit/6356138cefac2fd2aa46e0ca996e30d2ad6a31d9))
* Update the rebase editor spec ([ff68624](https://github.com/narnaud/git-loom/commit/ff68624f49d2b2775eefa45c48da320819a5d668))


### Changes

* Add utility methods for pre-condition, add consistent error messages ([b56af32](https://github.com/narnaud/git-loom/commit/b56af323e6ce17f61d468ddcfdf0e5dd999beb80))
* Improve code quality and error handling ([bde5218](https://github.com/narnaud/git-loom/commit/bde5218099a41a75f8463b8f66359fad663555fe))
* **reword:** Update with new shared API ([3a52d0a](https://github.com/narnaud/git-loom/commit/3a52d0a94e0bbd0729f8a295009d0fcab1412254))
* **update:** Do a fetch before a rebase ([dd8adbc](https://github.com/narnaud/git-loom/commit/dd8adbc02dc55c8b9acf42d103931cfa52b42cd8))


### Tests

* Refactor tests for better readability ([c140647](https://github.com/narnaud/git-loom/commit/c140647a6eddb5040bd5ccea293f19965a78b510))


### Other

* **claude:** Update CLAUDE.md ([b9bf8fc](https://github.com/narnaud/git-loom/commit/b9bf8fc570f22467b9a55c4e7c90ad3c3254577d))
* Reformat ([218f434](https://github.com/narnaud/git-loom/commit/218f4340390788975c4458b3f59d2e37b45c2ff7))

## [0.3.0](https://github.com/narnaud/git-loom/compare/v0.2.0...v0.3.0) (2026-02-13)


### Features ✨

* Add resolve method to resolve hash or shortid ([9b4c62e](https://github.com/narnaud/git-loom/commit/9b4c62e3ac2f00fcc32d7cc005416d4212ce421f))
* **rebase:** Add an internal sequence editor ([7cf44e3](https://github.com/narnaud/git-loom/commit/7cf44e39edc34df0f19626959f5ae5862e331fe4))
* **reword:** Add the reword command ([7fd4495](https://github.com/narnaud/git-loom/commit/7fd449565b7e2cce9508a6a8d4a2f56824ad1831))
* **reword:** Ask new branch name if not provided ([64b5165](https://github.com/narnaud/git-loom/commit/64b5165d1ae81e7aebba2ccec4749d4487b2b535))
* **reword:** Hide git output, except in case of error ([2ebe007](https://github.com/narnaud/git-loom/commit/2ebe007dcbbcf46528ab4fee66319fb35bc8aa9e))
* **shortid:** Branch name can be used instead of shortid ([091ea79](https://github.com/narnaud/git-loom/commit/091ea795abc29cde3661c990e48309548baea5db))


### Bug Fixes 🐞

* **status:** Fix display of loose commits with feature branch ([3f92239](https://github.com/narnaud/git-loom/commit/3f92239623453eb0d8104abcfca16ed48cbb3621))
* **status:** Handle co-located branch ([07cb8f1](https://github.com/narnaud/git-loom/commit/07cb8f1fa1c14e19cc7586d90e85a6d18496de07))
* Use shell-escape crate to escape path ([4771b60](https://github.com/narnaud/git-loom/commit/4771b609a35900500f3bb1d97fc6cd45aa4cdb74))
* Validate hash in rebase ([d4dea94](https://github.com/narnaud/git-loom/commit/d4dea940c12bbd27cde158e91f97ffa805dfff80))


### Documentation

* **reword:** Add specification for reword ([d536544](https://github.com/narnaud/git-loom/commit/d53654454c142c3b0d151087c4b3b08b1ca2767f))
* Update following new development ([d815376](https://github.com/narnaud/git-loom/commit/d815376c8e6f6a8b31a272a2997d2b01f3fb3348))
* Update specifications ([ebfc98b](https://github.com/narnaud/git-loom/commit/ebfc98ba085428fb6ed6ab132e0082af08d95100))


### Changes

* Add git_branch command ([d99b1dd](https://github.com/narnaud/git-loom/commit/d99b1ddf558a0861158ac9cddc77cc793c045e99))
* Extract `collect_entities` for reusability ([668a8d0](https://github.com/narnaud/git-loom/commit/668a8d05aa809be5a66358c3a91e278c68c1a61e))
* Extract git commands into their own module ([5e1cf9a](https://github.com/narnaud/git-loom/commit/5e1cf9a916f154662a45299af7765520749f94fd))
* Improve git rebase algorithm for Edit action ([2b02584](https://github.com/narnaud/git-loom/commit/2b025845e012e690aa8cc0fce22d462f1bf60ee1))
* **rebase:** Use json to pass actions for the rebase todo ([36e2a1a](https://github.com/narnaud/git-loom/commit/36e2a1a304c7a5382f31d75220536e9d461373be))


### Tests

* Add more helpers to simplify tests ([0c1d6eb](https://github.com/narnaud/git-loom/commit/0c1d6eb83a0dd8890a35009bf4c50f9b9ec733d4))


### Other

* **claude:** Update CLAUDE.md based on last development ([a40dfae](https://github.com/narnaud/git-loom/commit/a40dfae9553a28c964ab167b5d5ff3e330027de4))
* Reformat code ([42119a7](https://github.com/narnaud/git-loom/commit/42119a7525e8f1c0910db1fdf83b3cf0c16f5564))

## [0.2.0](https://github.com/narnaud/git-loom/compare/v0.1.0...v0.2.0) (2026-02-09)


### Features ✨

* **log:** Handle upstream ahead ([738ed62](https://github.com/narnaud/git-loom/commit/738ed626d15d0d909dab5697dee8b98ff78c6f2f))
* **log:** Implement log command ([3570e3e](https://github.com/narnaud/git-loom/commit/3570e3ee4354d106759787de2f26cc7a48b472ff))
* **shortid:** Compute and display shortids ([fb31236](https://github.com/narnaud/git-loom/commit/fb312369f115b25a23a19fa11baf8e18b71cf62e))
* **shortid:** Update computation again to keep them visually distinct ([2229275](https://github.com/narnaud/git-loom/commit/2229275890aafc9079c6e8bad3ba6cad6444c7f7))
* **status:** Add colors for better reading ([a0f5799](https://github.com/narnaud/git-loom/commit/a0f5799c5d429936841cd1fac72323cfdbf11cf5))
* **status:** Rename log command into status ([498d063](https://github.com/narnaud/git-loom/commit/498d06338f3004599cbeab83856060f4446a800f))


### Bug Fixes 🐞

* **log:** Fix how loose commits are displayed ([37df668](https://github.com/narnaud/git-loom/commit/37df668f5bbf100b41e15bde85ed7db39c792133))
* **status:** Better handling of errors ([7020a06](https://github.com/narnaud/git-loom/commit/7020a06e257458ae98667d9623c9f9a95d90eca8))
* **status:** Don't recompute hash ([359f70a](https://github.com/narnaud/git-loom/commit/359f70a9e8bfa92da4b099aaa2aae45f04d3a28f))
* **status:** Use chrono to extract date, add MSRV ([a66b669](https://github.com/narnaud/git-loom/commit/a66b6692f7a2da3fca7bd77ad5e8603a53d51e88))


### Performance Improvements ⚡

* **status:** Some performance improvements, better idiomatic rust ([25a9f05](https://github.com/narnaud/git-loom/commit/25a9f0567f2513c611fe7167e4b08e3edcc77987))


### Documentation

* Add README and LICENSE files ([fa21580](https://github.com/narnaud/git-loom/commit/fa21580509b830b7780d719ae5b68e734464a57f))
* Add specification for the log command ([55269f8](https://github.com/narnaud/git-loom/commit/55269f86cd6ba1f0f42e9006339006d2653a841d))
* **shortid:** Add specification for the shortid ([51ecd0f](https://github.com/narnaud/git-loom/commit/51ecd0fe0c37d44263b0acd62254f8c9f035fca7))
* update README file ([cd0253b](https://github.com/narnaud/git-loom/commit/cd0253bcb783a2685a9f20acb452c664718ca9cf))


### Changes

* **shortid:** Improve collision algorithm, to keep 2 letters ([ec45aa1](https://github.com/narnaud/git-loom/commit/ec45aa118cca8fd32218b134581cafd42fffb3a0))


### Tests

* **log:** Add unit tests for the git extraction ([5a6da20](https://github.com/narnaud/git-loom/commit/5a6da20fddd28a9793557354a3c9e2ef0dc2959d))
* **log:** Add unit tests for the graph ([b19156e](https://github.com/narnaud/git-loom/commit/b19156e1c51f0ffe7f745bed78d3b40d199bbd6f))


### Other

* Add pre-commit ([a7d6844](https://github.com/narnaud/git-loom/commit/a7d684461a7e9702f0e9a45a4c9fef8c3136a315))
* Add release-please actions ([48c7996](https://github.com/narnaud/git-loom/commit/48c79966e5d849781c885d1ea085cac3801e94fb))
* **claude:** Add Claude settings ([94e93bc](https://github.com/narnaud/git-loom/commit/94e93bcd978e36f3c9e9ca757e1199bb0bde0e26))
* **claude:** Add CLAUDE.md file ([3e1e427](https://github.com/narnaud/git-loom/commit/3e1e427080ab3521eca4c0344873e9d8ede24465))
* **claude:** Add code-reviewer agent ([1a6ffdc](https://github.com/narnaud/git-loom/commit/1a6ffdc415ff8db4019ce3ca83992e31a656e752))
