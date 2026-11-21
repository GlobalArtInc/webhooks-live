use whl_script::{PoolConfig, PoolError, ScriptPool, allocator_installed};

#[test]
fn the_pool_refuses_to_start_without_the_counting_allocator() {
    assert!(!allocator_installed());
    assert!(matches!(
        ScriptPool::new(PoolConfig::default()),
        Err(PoolError::AllocatorNotInstalled)
    ));
}
