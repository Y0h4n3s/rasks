# Rasks

## Installation

add this to your Cargo.toml dependencies

```toml
rasks = "0.1.0"
```

# Usage

```rust
use std::collections::HashMap;
use rasks::{Arg, ExecutionSchedule, Executor, MemoryExecutor, Result};


pub struct Arg1 {
    pub a: String,
}

fn main() {
    let mut executor = MemoryExecutor::new();

    let task = |args: &Vec<Arg>| {
        // downcast your args
        if let (Some(mut arg1), Ok(_arg2)) = (
            args[0].0.downcast_ref::<Arc<Mutex<Arg1>>>(),
            args[1].0.downcast::<u32>()) {
            // execute your tasks logic here
            let arg = arg1.lock().expect("poisoned mutex");
            *arg = "new".to_string();
        }

        // task closures should return anyhow::Result<()>
        Ok(())
    };

    let clonable_state = Arc::new(Mutex::new(Arg1{a: 1}));
    executor.launch(
        task,
        vec![
            Arg(Box::new(clonable_state)),
            Arg(Box::new(1))
        ],
        ExecutionSchedule::Every(Duration::from_secs(10))
    );

    executor.join().unwrap()
}
```