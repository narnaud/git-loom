use super::*;
use crate::core::test_helpers::TestRepo;

#[test]
fn a_tab_is_as_wide_as_configured_and_four_spaces_when_nothing_usable_is() {
    let test_repo = TestRepo::new();
    let width = || tab_width(&git2::Repository::open(test_repo.workdir()).unwrap());

    test_repo.set_config("loom.tuiTabWidth", "8");
    assert_eq!(width(), 8);

    test_repo.set_config("loom.tuiTabWidth", "0");
    assert_eq!(width(), 4);
}
