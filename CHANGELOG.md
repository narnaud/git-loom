# Changelog

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

* Remove architecture details from the specificaitons ([75ba39f](https://github.com/narnaud/git-loom/commit/75ba39f33ccc9f3123d6ece4fde2397efbc9192d))
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
* **update:** Add udpate command ([892322e](https://github.com/narnaud/git-loom/commit/892322e28c90e91d0c99af9a37805ba55d7cb07c))


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
* Upate documentaiton to match code ([8a89878](https://github.com/narnaud/git-loom/commit/8a898785ade47bbb1bba0b8b77692036ba5bfe3d))
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
