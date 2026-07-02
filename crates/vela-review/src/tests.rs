use super::*;

#[test]
fn generate_candidates_skips_duplicate_memory_work_on_repeat_passes() {
    let vela_home =
        std::env::temp_dir().join(format!("vela-review-test-{}", unix_timestamp_nanos()));
    vela_memory::initialize_memory(&vela_home).unwrap();
    vela_skills::initialize_skills(&vela_home).unwrap();
    initialize_reviews(&vela_home).unwrap();

    let input = SuggestionInput {
        session_id: "session-1".to_string(),
        session_title: "Session 1".to_string(),
        messages: vec![SuggestionMessage {
            role: "user".to_string(),
            content: "Please remember that I prefer concise responses.".to_string(),
        }],
        events: vec![],
    };

    let first = generate_candidates(&vela_home, input.clone()).unwrap();
    assert_eq!(first.candidate_ids.len(), 1);

    let second = generate_candidates(&vela_home, input).unwrap();
    assert!(second.candidate_ids.is_empty());
    assert!(second.skipped >= 1);

    fs::remove_dir_all(&vela_home).unwrap();
}
