//! Conformance suite — same set of cases runs against fs + memory.
//! MinIO via testcontainers comes online when Docker is part of CI.

use std::time::Duration;

use bytes::Bytes;
use drive_storage::{SignedUrl, Storage, StorageError};
use futures::TryStreamExt;
use tempfile::TempDir;

fn key() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(7);
    }
    k
}

enum Backend {
    Fs(TempDir),
    Memory,
}

fn make(b: &Backend) -> Storage {
    match b {
        Backend::Fs(td) => Storage::fs(td.path().to_string_lossy().into_owned(), key()).unwrap(),
        Backend::Memory => Storage::memory(key()).unwrap(),
    }
}

async fn put_get_roundtrip(b: Backend) {
    let s = make(&b);
    let body = Bytes::from_static(b"hello world");
    let meta = s.put("dir/file.txt", body.clone(), None).await.unwrap();
    assert_eq!(meta.size, body.len() as u64);
    let (_m, stream) = s.get("dir/file.txt", None).await.unwrap();
    let chunks: Vec<Bytes> = stream.try_collect().await.unwrap();
    let total: Vec<u8> = chunks.into_iter().flatten().collect();
    assert_eq!(total, body.as_ref());
}

async fn stat_then_delete(b: Backend) {
    let s = make(&b);
    s.put("k", Bytes::from_static(b"x"), None).await.unwrap();
    assert_eq!(s.stat("k").await.unwrap().size, 1);
    s.delete("k").await.unwrap();
    assert!(matches!(
        s.stat("k").await.unwrap_err(),
        StorageError::NotFound(_)
    ));
}

async fn copy_then_rename(b: Backend) {
    let s = make(&b);
    s.put("src.txt", Bytes::from_static(b"hi"), None)
        .await
        .unwrap();
    s.copy("src.txt", "copy.txt").await.unwrap();
    s.rename("copy.txt", "renamed.txt").await.unwrap();
    assert!(matches!(
        s.stat("copy.txt").await.unwrap_err(),
        StorageError::NotFound(_)
    ));
    assert_eq!(s.stat("renamed.txt").await.unwrap().size, 2);
}

async fn signed_get_round_trip(b: Backend) {
    let s = make(&b);
    s.put("t.txt", Bytes::from_static(b"sig"), None)
        .await
        .unwrap();
    let url = s
        .signed_get("t.txt", Duration::from_secs(60))
        .await
        .unwrap();
    let SignedUrl::Token { token, .. } = url else {
        panic!("fs/memory must use Token variant")
    };
    let (key, method) = s.verify_token(&token).unwrap();
    assert_eq!(key, "t.txt");
    assert_eq!(method, "GET");
}

async fn signed_token_rejects_tamper(b: Backend) {
    let s = make(&b);
    s.put("t", Bytes::from_static(b"x"), None).await.unwrap();
    let SignedUrl::Token { token, .. } = s.signed_get("t", Duration::from_secs(60)).await.unwrap()
    else {
        panic!()
    };
    let mut bad = token.clone();
    let last = bad.len() - 1;
    let ch = bad.chars().last().unwrap();
    let new = if ch == 'A' { 'B' } else { 'A' };
    bad.replace_range(last..last + 1, &new.to_string());
    assert!(matches!(
        s.verify_token(&bad).unwrap_err(),
        StorageError::InvalidToken
    ));
}

async fn invalid_keys_rejected(b: Backend) {
    let s = make(&b);
    for bad in ["", "../escape", "/abs", "back\\slash", "null\0byte"] {
        assert!(matches!(
            s.put(bad, Bytes::from_static(b"x"), None)
                .await
                .unwrap_err(),
            StorageError::InvalidKey(_)
        ));
    }
}

fn fs() -> Backend {
    Backend::Fs(TempDir::new().unwrap())
}

#[tokio::test]
async fn fs_put_get() {
    put_get_roundtrip(fs()).await;
}
#[tokio::test]
async fn mem_put_get() {
    put_get_roundtrip(Backend::Memory).await;
}
#[tokio::test]
async fn fs_stat_delete() {
    stat_then_delete(fs()).await;
}
#[tokio::test]
async fn mem_stat_delete() {
    stat_then_delete(Backend::Memory).await;
}
#[tokio::test]
async fn fs_copy_rename() {
    copy_then_rename(fs()).await;
}
#[tokio::test]
async fn mem_copy_rename() {
    copy_then_rename(Backend::Memory).await;
}
#[tokio::test]
async fn fs_signed() {
    signed_get_round_trip(fs()).await;
}
#[tokio::test]
async fn mem_signed() {
    signed_get_round_trip(Backend::Memory).await;
}
#[tokio::test]
async fn fs_signed_tamper() {
    signed_token_rejects_tamper(fs()).await;
}
#[tokio::test]
async fn mem_signed_tamper() {
    signed_token_rejects_tamper(Backend::Memory).await;
}
#[tokio::test]
async fn fs_invalid_keys() {
    invalid_keys_rejected(fs()).await;
}
#[tokio::test]
async fn mem_invalid_keys() {
    invalid_keys_rejected(Backend::Memory).await;
}
