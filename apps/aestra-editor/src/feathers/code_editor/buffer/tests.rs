use super::*;

#[test]
fn unicode_selection_undo_redo_restore_exact_text_caret_and_anchor() {
    let mut editor = CodeEditor::new("é火🙂 fin".into()).with_selection(3, 1);
    editor.insert_for_test("煙");
    assert_eq!(editor.text, "é煙 fin");
    assert_eq!((editor.cursor, editor.anchor), (2, 2));
    assert!(editor.undo());
    assert_eq!(editor.text, "é火🙂 fin");
    assert_eq!((editor.cursor, editor.anchor), (3, 1));
    assert!(editor.redo());
    assert_eq!(editor.text, "é煙 fin");
}

#[test]
fn bounded_history_keeps_latest_edits_and_new_edits_fork_redo() {
    let mut editor = CodeEditor::new(String::new());
    for _ in 0..MAX_UNDO + 10 {
        editor.insert_for_test("a");
    }
    assert_eq!(editor.undo.len(), MAX_UNDO);
    for _ in 0..MAX_UNDO {
        assert!(editor.undo());
    }
    assert!(!editor.undo());
    assert_eq!(editor.text, "a".repeat(10));
    assert!(editor.redo());
    editor.insert_for_test("火");
    assert!(!editor.can_redo());
}

#[test]
fn typing_coalesces_but_undo_and_other_edits_break_the_run() {
    let mut editor = CodeEditor::new(String::new());
    for text in ["火", "花"] {
        editor.record(EditKind::Type);
        editor.insert(text);
    }
    assert_eq!(editor.undo.len(), 1);
    editor.insert_for_test("!");
    assert!(editor.undo());
    assert_eq!(editor.text, "火花");
    assert!(editor.undo());
    assert!(editor.text.is_empty());
    editor.record(EditKind::Type);
    editor.insert("新");
    assert_eq!(editor.undo.len(), 1);
    assert!(!editor.can_redo());
}

#[test]
fn buffer_clones_have_independent_stacks_and_noop_steps_preserve_selection() {
    let mut original = CodeEditor::new("abc".into()).with_selection(99, 99);
    assert_eq!((original.cursor, original.anchor), (3, 3));
    assert!(!original.undo());
    assert!(!original.redo());
    original.insert_for_test("d");
    let mut cloned = original.clone();
    assert!(cloned.undo());
    assert_eq!(cloned.text, "abc");
    assert_eq!(original.text, "abcd");
    assert!(original.can_undo());
    assert!(!original.can_redo());
}

#[test]
fn character_line_mapping_is_independent_of_utf8_byte_width() {
    let editor = CodeEditor::new("é火\n🙂x\nfin".into());
    assert_eq!(line_col(&editor.text, 2), (0, 2));
    assert_eq!(line_col(&editor.text, 4), (1, 1));
    assert_eq!(editor.line_of(99), 2);
}
