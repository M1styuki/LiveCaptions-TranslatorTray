use super::*;
const FIRST: &str = "This is the first public sentence before deletion.";
const SECOND: &str = "This is the second public sentence being translated.";
const THIRD: &str = "This is a genuinely new public sentence after deletion.";
fn stored(fixture: &mut Fixture) {
    fixture.capture(FIRST.into(), 1, 0);
    fixture.until(|fixture| fixture.network.len() == 1);
    fixture.finish(0, Some(Ok("保存的公开译文".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 1);
}
fn cleared(fixture: &mut Fixture) {
    fixture.until(|fixture| !fixture.flow.test_clearing() && fixture.flow.pending_saves() == 0);
    assert!(fixture.flow.take_clear_result().unwrap().is_ok());
}
#[test]
fn deletion_keeps_new_short_sentence_clean_and_retranslates_it_after_configuration_change() {
    let mut fixture = Fixture::new(false);
    fixture.capture("你好。".into(), 1, 0);
    fixture.until(|fixture| fixture.network.len() == 1);
    fixture.finish(0, Some(Ok("old".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    fixture.flow.clear_history();
    cleared(&mut fixture);
    fixture.capture("你好。再见。".into(), 2, 0);
    fixture.until(|fixture| fixture.network.len() == 1);
    assert_eq!(fixture.network[0].task.source, "再见。");
    let previous_key = fixture.network[0].task.key.clone();
    fixture.finish(0, Some(Ok("new".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let screen = Screen::new(1920.0, 1080.0).unwrap();
    let settings = Settings::defaults(screen)
        .edited(screen, |value| value["TargetLanguage"] = "ja-JP".into())
        .unwrap()
        .0;
    fixture.flow.reconfigure(settings, false);
    fixture.until(|fixture| fixture.network.len() == 1);
    assert_eq!(fixture.network[0].task.source, "再见。");
    assert_ne!(
        fixture.network[0].task.key.sentence_id,
        previous_key.sentence_id
    );
    assert_ne!(
        fixture.network[0].task.key.config_revision,
        previous_key.config_revision
    );
    fixture.finish(0, Some(Ok("再翻訳".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let page = fixture.page();
    assert_eq!(page.total, 2);
    assert!(page.rows.iter().all(|row| row.source == "再见。"));
    assert!(
        page.rows
            .iter()
            .any(|row| row.translation == "再翻訳" && row.target_language == "ja-JP")
    );
    assert!(
        page.rows
            .iter()
            .any(|row| row.translation == "new" && row.target_language == "zh-CN")
    );
    fixture.close();
}
#[test]
fn deletion_keeps_the_already_visible_short_unfinished_tail_suppressed_while_it_grows() {
    let mut fixture = Fixture::new(false);
    stored(&mut fixture);
    fixture.capture(format!("{FIRST} Hi"), 2, 0);
    fixture.flow.clear_history();
    cleared(&mut fixture);
    fixture.capture(format!("{FIRST} Hi there, this was already speaking"), 3, 0);
    fixture.capture(
        format!("{FIRST} Hi there, this was already speaking before deletion."),
        4,
        0,
    );
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    fixture.pump();
    assert!(
        fixture.network.is_empty(),
        "the already visible logical current sentence must not reappear after crossing the short-text extraction threshold"
    );
    assert_eq!(fixture.page().total, 0);
    fixture.capture(
        format!("{FIRST} Hi there, this was already speaking before deletion. {THIRD}"),
        5,
        0,
    );
    fixture.until(|fixture| fixture.network.len() == 1);
    fixture.finish(0, Some(Ok("真正下一句".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let page = fixture.page();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].source.trim(), THIRD);
    fixture.close();
}
#[test]
fn deletion_rejects_old_reply_and_current_revision_but_keeps_future_sentences() {
    let mut fixture = Fixture::new(false);
    stored(&mut fixture);
    fixture.capture(format!("{FIRST} {SECOND}"), 2, 0);
    fixture.until(|fixture| fixture.network.len() == 1);
    fixture.flow.clear_history();
    assert_eq!(fixture.flow.test_context_count(), 0);
    cleared(&mut fixture);
    fixture.finish(0, Some(Ok("过期回包不得保存".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 0);
    fixture.capture(format!("{FIRST} {SECOND}"), 2, 50);
    assert!(fixture.network.is_empty());
    let revised = "This is the second public sentence being translated correctly.";
    fixture.capture(format!("{FIRST} {revised}"), 3, 0);
    assert!(fixture.network.is_empty());
    assert_eq!(fixture.page().total, 0);
    let screen = Screen::new(1920.0, 1080.0).unwrap();
    let settings = Settings::defaults(screen)
        .edited(screen, |value| value["TargetLanguage"] = "ja-JP".into())
        .unwrap()
        .0;
    fixture.flow.reconfigure(settings, false);
    fixture.until(|fixture| !fixture.flow.test_sample_pending() && fixture.flow.test_fence_ready());
    assert!(
        fixture.network.is_empty(),
        "configuration change cannot replay the deleted current sentence"
    );
    fixture.capture(format!("{FIRST} {revised} {THIRD}"), 4, 0);
    fixture.until(|fixture| fixture.network.len() == 1);
    assert_eq!(fixture.flow.test_context_count(), 0);
    fixture.finish(0, Some(Ok("新公开译文".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let page = fixture.page();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].source.trim(), THIRD);
    assert_eq!(page.rows[0].translation, "新公开译文");
    fixture.close();
}
#[test]
fn deletion_waits_for_failed_accepted_save_to_become_durable_before_deleting() {
    let mut fixture = Fixture::new(false);
    stored(&mut fixture);
    fixture.capture(format!("{FIRST} {SECOND}"), 2, 0);
    fixture.until(|fixture| fixture.network.len() == 1);
    let lock =
        rusqlite::Connection::open(fixture.root.join("data/translation_history.db")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    fixture.allow_blocked = true;
    fixture.finish(0, Some(Ok("保存失败仍需持有的译文".into())));
    fixture.flow.clear_history();
    fixture.until(|fixture| fixture.flow.is_blocked());
    assert!(
        !fixture.flow.test_delete_submitted(),
        "submitted save is not a durable acknowledgement"
    );
    assert!(fixture.flow.test_clearing());
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    assert_eq!(fixture.page().total, 1);
    fixture.flow.retry();
    fixture.allow_blocked = false;
    cleared(&mut fixture);
    assert_eq!(fixture.page().total, 0);
    fixture.close();
}
#[test]
fn deletion_failure_rolls_back_records_without_automatic_retry() {
    let mut fixture = Fixture::new(false);
    stored(&mut fixture);
    let lock =
        rusqlite::Connection::open(fixture.root.join("data/translation_history.db")).unwrap();
    lock.execute_batch("CREATE TRIGGER fixture_delete_failure BEFORE DELETE ON TranslationHistory BEGIN SELECT RAISE(ABORT,'controlled fixture deletion failure'); END;").unwrap();
    fixture.flow.clear_history();
    fixture.until(|fixture| !fixture.flow.test_clearing() && fixture.flow.pending_saves() == 0);
    assert!(fixture.flow.take_clear_result().unwrap().is_err());
    assert!(!fixture.flow.is_blocked());
    assert_eq!(fixture.page().total, 1);
    fixture.capture(FIRST.into(), 1, 50);
    assert!(fixture.network.is_empty());
    assert_eq!(fixture.page().total, 1);
    lock.execute_batch("DROP TRIGGER fixture_delete_failure")
        .unwrap();
    drop(lock);
    fixture.flow.clear_history();
    cleared(&mut fixture);
    assert_eq!(fixture.page().total, 0);
    fixture.close();
}
#[test]
fn deletion_keeps_observed_new_captions_received_while_database_is_busy() {
    let mut fixture = Fixture::new(false);
    stored(&mut fixture);
    let lock =
        rusqlite::Connection::open(fixture.root.join("data/translation_history.db")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    fixture.flow.clear_history();
    fixture.pump();
    assert!(fixture.flow.test_delete_submitted());
    for (changes, text) in [
        (2, format!("{FIRST} {SECOND}")),
        (3, format!("{FIRST} {SECOND} {THIRD}")),
    ] {
        fixture.queue_capture(text, changes, 0);
        fixture.pump();
    }
    assert!(fixture.network.is_empty());
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    fixture.until(|fixture| fixture.network.len() == 2);
    assert!(fixture.flow.take_clear_result().unwrap().is_ok());
    fixture.finish(0, Some(Ok("删除后第二句".into())));
    fixture.finish(0, Some(Ok("删除后第三句".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 2);
    fixture.close();
}
#[test]
fn deletion_suppresses_short_current_revision_without_suppressing_next_complete_sentence() {
    let mut fixture = Fixture::new(true);
    fixture.capture("你好。".into(), 1, 0);
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 1);
    fixture.flow.clear_history();
    cleared(&mut fixture);
    fixture.capture("你好。".into(), 1, 50);
    fixture.capture("你好啊。".into(), 2, 0);
    assert_eq!(fixture.page().total, 0);
    fixture.capture("你好啊。再见。".into(), 3, 0);
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let page = fixture.page();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].source, "再见。");
    fixture.capture("你好啊。再见。好了。".into(), 4, 0);
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    let page = fixture.page();
    assert_eq!(page.total, 2);
    assert!(page.rows.iter().all(|row| !row.source.contains("你好")));
    assert!(page.rows.iter().any(|row| row.source.contains("好了。")));
    fixture.close();
}
#[test]
fn deletion_survives_engine_stop_until_database_acknowledges_it() {
    let mut fixture = Fixture::new(false);
    stored(&mut fixture);
    fixture.flow.clear_history();
    fixture.flow.stop();
    assert!(fixture.flow.pending_saves() > 0);
    cleared(&mut fixture);
    assert_eq!(fixture.page().total, 0);
    fixture.close();
}

#[test]
fn deletion_keeps_a_genuinely_new_repeated_sentence_when_the_transcript_rolls() {
    let mut fixture = Fixture::new(false);
    fixture.capture("Old context. This sentence is repeated.".into(), 1, 0);
    fixture.until(|fixture| fixture.network.len() == 1);
    fixture.finish(0, Some(Ok("删除前的句子".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    fixture.flow.clear_history();
    cleared(&mut fixture);
    fixture.capture(
        "This sentence is repeated. This sentence is repeated.".into(),
        2,
        0,
    );
    fixture.until(|fixture| fixture.network.len() == 1);
    assert_eq!(
        fixture.network[0].task.source.trim(),
        "This sentence is repeated."
    );
    fixture.finish(0, Some(Ok("真正新说的重复句".into())));
    fixture.until(|fixture| fixture.flow.pending_saves() == 0);
    assert_eq!(fixture.page().total, 1);
    fixture.close();
}
