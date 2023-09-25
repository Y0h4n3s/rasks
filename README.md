# Rasks

## Installation

add this to your Cargo.toml dependencies

```toml
rasks = "0.2.0"
```

# Usage

```rust
use rasks::{ExecutionSchedule, Executor, MemoryExecutor};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Arg1 {
    pub a: String,
}

fn main() {
    let mut executor = MemoryExecutor::new();

    let task = |(arg1, _): &(Arc<Mutex<&str>>, u32)| {
        let mut arg = arg1.lock().expect("poisoned mutex");
        *arg = "new";
        println!("Running @ {:?}", Instant::now());
        // task closures should return anyhow::Result<()>
        Ok(())
    };

    let clonable_state = Arc::new(Mutex::new("old"));
    let task_id = executor
        .launch(
            task,
            (clonable_state, 1),
            ExecutionSchedule::Every(Duration::from_secs(10)),
            None,
        )
        .unwrap();
    executor.join_task(&task_id).unwrap()
}
```

## Async
If your app is running on the tokio runtime you can use the async executor
```rust
use rasks::{BoxPinnedFuture, ExecutionSchedule, AsyncExecutor, AsyncMemoryExecutor};
use std::time::Duration;
use std::sync::Arc;

#[derive(Clone)]
pub struct Arg1 {
    pub a: i32,
}


pub struct Arg2 {
    pub a: i32,
}
#[tokio::main]
async fn main() {
    let mut executor = AsyncMemoryExecutor::new();
    fn task1((arg1, arg2): &(Arc<Arg1>, Arg2)) -> BoxPinnedFuture<'_> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_secs(2)).await;
            assert_eq!(arg1.a, 10);
            assert_eq!(arg2.a, 20);
            Ok(())
        })
    }

    let task_id = executor
        .launch(
            task1,
            (Arc::new(Arg1 { a: 10 }), Arg2 { a: 20 }),
            ExecutionSchedule::Once(Duration::from_secs(2), false),
            None,
        )
        .await
        .unwrap();

    executor.join_task(&task_id).await.unwrap();
    println!("Done");

}
```
