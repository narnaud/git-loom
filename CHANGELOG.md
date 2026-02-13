# Changelog

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
