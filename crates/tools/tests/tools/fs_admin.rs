// ---- fs admin -------------------------------------------------------------

#[tokio::test]
async fn fs_admin_roundtrip() {
    let (ws, dir) = temp_ws();
    let write = WriteFileTool::new(ws.clone());
    write
        .call(json!({"path": "a.txt", "content": "data"}), &ctx())
        .await
        .unwrap();

    let mkdir = CreateFolderTool::new(ws.clone());
    mkdir
        .call(json!({"path": "nested/dir"}), &ctx())
        .await
        .unwrap();
    assert!(dir.join("nested/dir").is_dir());

    let copy = CopyFileTool::new(ws.clone());
    let out = copy
        .call(json!({"from": "a.txt", "to": "nested/b.txt"}), &ctx())
        .await
        .unwrap();
    assert_eq!(out["bytesCopied"], json!(4));

    let rename = RenameFileTool::new(ws.clone());
    rename
        .call(json!({"from": "nested/b.txt", "to": "c.txt"}), &ctx())
        .await
        .unwrap();
    assert!(dir.join("c.txt").exists());
    assert!(!dir.join("nested/b.txt").exists());

    let info = FileInfoTool::new(ws.clone());
    let meta = info.call(json!({"path": "c.txt"}), &ctx()).await.unwrap();
    assert_eq!(meta["exists"], json!(true));
    assert_eq!(meta["kind"], json!("file"));
    assert_eq!(meta["sizeBytes"], json!(4));
    let missing = info.call(json!({"path": "nope"}), &ctx()).await.unwrap();
    assert_eq!(missing["exists"], json!(false));

    let del = DeleteFileTool::new(ws.clone());
    // Directory without recursive is refused.
    let err = del
        .call(json!({"path": "nested"}), &ctx())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("recursive"));
    del.call(json!({"path": "nested", "recursive": true}), &ctx())
        .await
        .unwrap();
    assert!(!dir.join("nested").exists());
    del.call(json!({"path": "c.txt"}), &ctx()).await.unwrap();
    assert!(!dir.join("c.txt").exists());
    std::fs::remove_dir_all(&dir).ok();
}
