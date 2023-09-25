use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

use anyhow::Result;

pub enum ExecutionSchedule {
    Every(Duration),
    Once(Duration, bool),
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Already ran")]
    AlreadyRan,

    #[error("A task with the same id already exists")]
    DuplicateId,

    #[error("there is no task with id {0}")]
    UnknownId(String),
}

pub trait ExecutionResult {
    type Type;
    fn get(&self) -> Self::Type;
}

pub trait Executor {
    fn launch<T, A>(
        &mut self,
        task: T,
        args: A,
        schedule: ExecutionSchedule,
        id: Option<&str>,
    ) -> Result<String>
    where
        A: Send + 'static,
        T: Fn(&A) -> Result<()> + Send + Sync + 'static;

    fn join_task(&mut self, task_id: &str) -> Result<()>;

    fn join(self) -> Result<()>;
}

#[async_trait::async_trait]
pub trait AsyncExecutor {
    async fn launch<T, A>(
        &mut self,
        task: T,
        args: A,
        schedule: ExecutionSchedule,
        id: Option<&str>,
    ) -> Result<String>
    where
        A: Send + Sync + 'static,

        T: Fn(&A) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + '_>>
            + Send
            + Sync
            + 'static;

    async fn join_task(&mut self, task_id: &str) -> Result<()>;
    async fn join(self) -> Result<()>;
}

#[derive(Default)]
pub struct MemoryExecutor {
    tasks: HashMap<String, JoinHandle<Result<()>>>,
}

impl MemoryExecutor {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }
}
#[derive(Default)]

pub struct AsyncMemoryExecutor {
    tasks: HashMap<String, tokio::task::JoinHandle<Result<()>>>,
}

impl AsyncMemoryExecutor {
    pub fn new() -> Self {
        // panic if there is no tokio runtime
        tokio::runtime::Handle::current();
        Self {
            tasks: HashMap::new(),
        }
    }
}
impl ExecutionSchedule {
    fn next_tick(&mut self) -> Result<()> {
        match *self {
            ExecutionSchedule::Every(n) => {
                thread::sleep(n);
                Ok(())
            }
            ExecutionSchedule::Once(n, ran) => {
                if ran {
                    return Err(Error::AlreadyRan.into());
                }
                thread::sleep(n);
                *self = ExecutionSchedule::Once(n, true);
                Ok(())
            }
        }
    }
}

impl Executor for MemoryExecutor {
    fn launch<T, A>(
        &mut self,
        task: T,
        args: A,
        schedule: ExecutionSchedule,
        id: Option<&str>,
    ) -> Result<String>
    where
        A: Send + 'static,
        T: Fn(&A) -> Result<()> + Send + Sync + 'static,
    {
        let task_id = if let Some(id) = id {
            if self.tasks.contains_key(id) {
                return Err(Error::DuplicateId.into());
            }
            id.to_string()
        } else {
            #[allow(unused_assignments)]
            let mut id = "".to_string();
            loop {
                id = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
                    .to_string();
                if self.tasks.contains_key(&id) {
                    continue;
                }
                break;
            }
            id
        };
        self.tasks.insert(
            task_id.clone(),
            thread::spawn(move || {
                let mut schedule = schedule;
                while schedule.next_tick().is_ok() {
                    task(&args)?;
                }
                Ok::<(), anyhow::Error>(())
            }),
        );
        Ok(task_id)
    }

    // TODO: Handle errors better here
    fn join_task(&mut self, task_id: &str) -> Result<()> {
        if !self.tasks.contains_key(task_id) {
            return Err(Error::UnknownId(task_id.to_string()).into());
        }
        if let Some(task) = self.tasks.remove(task_id) {
            task.join().unwrap().unwrap()
        }
        Ok(())
    }
    fn join(self) -> Result<()> {
        for (_, task) in self.tasks {
            task.join().unwrap().unwrap()
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AsyncExecutor for AsyncMemoryExecutor {
    async fn launch<T, A>(
        &mut self,
        task: T,
        args: A,
        schedule: ExecutionSchedule,
        id: Option<&str>,
    ) -> Result<String>
    where
        A: Send + Sync + 'static,
        T: Fn(&A) -> Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + '_>>
            + Send
            + Sync
            + 'static,
    {
        let task_id = if let Some(id) = id {
            if self.tasks.contains_key(id) {
                return Err(Error::DuplicateId.into());
            }
            id.to_string()
        } else {
            #[allow(unused_assignments)]
            let mut id = "".to_string();
            loop {
                id = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
                    .to_string();
                if self.tasks.contains_key(&id) {
                    continue;
                }
                break;
            }
            id
        };
        let runtime = tokio::runtime::Handle::current();
        self.tasks.insert(
            task_id.clone(),
            runtime.spawn(async move {
                let mut schedule = schedule;
                while schedule.next_tick().is_ok() {
                    task(&args).await?;
                }
                Ok::<(), anyhow::Error>(())
            }),
        );
        Ok(task_id)
    }

    async fn join_task(&mut self, task_id: &str) -> Result<()> {
        if !self.tasks.contains_key(task_id) {
            return Err(Error::UnknownId(task_id.to_string()).into());
        }
        if let Some(task) = self.tasks.remove(task_id) {
            task.await??
        }
        Ok(())
    }
    async fn join(self) -> Result<()> {
        for (_, task) in self.tasks {
            task.await??
        }
        Ok(())
    }
}

pub type BoxPinnedFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'a>>;

#[cfg(test)]
mod test {
    use crate::{
        AsyncExecutor, AsyncMemoryExecutor, BoxPinnedFuture, ExecutionSchedule, Executor,
        MemoryExecutor, Result,
    };
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Clone)]
    pub struct Arg1 {
        pub a: i32,
    }

    #[derive(Clone)]
    pub struct Arg2 {
        pub a: i32,
    }

    #[test]
    fn memory_executor_mutex_arg() {
        let mut executor = MemoryExecutor::new();

        let task = |(arg1, _): &(Arc<Mutex<&str>>, u32)| {
            let mut arg = arg1.lock().expect("poisoned mutex");
            *arg = "new";

            // task closures should return anyhow::Result<R>
            Ok(())
        };

        let clonable_state = Arc::new(Mutex::new("old"));
        let task_id = executor
            .launch(
                task,
                (clonable_state, 1),
                ExecutionSchedule::Once(Duration::from_secs(3), false),
                None,
            )
            .unwrap();
        executor.join_task(&task_id).unwrap()
    }

    #[test]
    fn memory_executor() {
        let mut executor = MemoryExecutor::new();
        let task = |(arg1, arg2): &(Arc<Arg1>, Arg2)| {
            assert_eq!(arg1.a, 1);
            assert_eq!(arg2.a, 2);
            Ok(())
        };

        fn task1((arg1, arg2): &(Arc<Arg1>, Arg2)) -> Result<()> {
            assert_eq!(arg1.a, 10);
            assert_eq!(arg2.a, 20);
            Ok(())
        }

        executor
            .launch(
                task,
                (Arc::new(Arg1 { a: 1 }), Arg2 { a: 2 }),
                ExecutionSchedule::Once(Duration::from_secs(1), false),
                None,
            )
            .unwrap();

        executor
            .launch(
                task1,
                (Arc::new(Arg1 { a: 10 }), Arg2 { a: 20 }),
                ExecutionSchedule::Once(Duration::from_secs(2), false),
                None,
            )
            .unwrap();

        executor.join().unwrap();
    }
    #[tokio::test]
    async fn async_memory_executor() {
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
    }
}
