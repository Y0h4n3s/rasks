use std::any::Any;
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
}

pub struct Arg(Box<dyn Any + Send + Sync>);

pub trait ExecutionResult {
    type Type;
    fn get(&self) -> Self::Type;
}

pub trait Executor {
    fn launch<T, R>(
        &mut self,
        task: T,
        args: Vec<Arg>,
        schedule: ExecutionSchedule,
        id: Option<&str>,
    ) -> Result<String>
    where
        T: Fn(&[Arg]) -> Result<R> + Send + Sync + 'static;
    fn join(self) -> Result<()>;
}

#[async_trait::async_trait]
pub trait AsyncExecutor {
    async fn launch<T, R>(
        &mut self,
        task: T,
        args: Vec<Arg>,
        schedule: ExecutionSchedule,
        id: Option<&str>,
    ) -> Result<String>
    where
        T: Fn(&[Arg]) -> Pin<Box<dyn Future<Output = Result<R>> + Send + Sync + '_>>
            + Send
            + Sync
            + 'static;

    async fn join_task(&self, task_id: &str) -> Result<()>;
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
    fn launch<T, R>(
        &mut self,
        task: T,
        args: Vec<Arg>,
        schedule: ExecutionSchedule,
        id: Option<&str>,
    ) -> Result<String>
    where
        T: Fn(&[Arg]) -> Result<R> + Send + Sync + 'static,
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
                    task(args.as_slice())?;
                }
                Ok::<(), anyhow::Error>(())
            }),
        );
        Ok(task_id)
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
    async fn launch<T, R>(
        &mut self,
        task: T,
        args: Vec<Arg>,
        schedule: ExecutionSchedule,
        id: Option<&str>,
    ) -> Result<String>
    where
        T: Fn(&[Arg]) -> Pin<Box<dyn Future<Output = Result<R>> + Send + Sync + '_>>
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

    async fn join_task(&self, _task_id: &str) -> Result<()> {
        unimplemented!()
    }
    async fn join(self) -> Result<()> {
        for (_, task) in self.tasks {
            task.await.unwrap().unwrap()
        }
        Ok(())
    }
}

pub type BoxPinnedFuture<'a, R> = Pin<Box<dyn Future<Output = R> + Send + Sync + 'a>>;

#[cfg(test)]
mod test {
    use crate::{
        Arg, AsyncExecutor, AsyncMemoryExecutor, BoxPinnedFuture, ExecutionSchedule, Executor,
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

        let task = |args: &[Arg]| {
            // downcast your args
            if let (Some(arg1), Some(_arg2)) = (
                args[0].0.downcast_ref::<Arc<Mutex<String>>>(),
                args[1].0.downcast_ref::<u32>(),
            ) {
                // execute your tasks logic here
                let mut arg = arg1.lock().expect("poisoned mutex");
                *arg = "new".to_string();
            }

            // task closures should return anyhow::Result<()>
            Ok(())
        };

        let clonable_state = Arc::new(Mutex::new("old"));
        executor
            .launch(
                task,
                vec![Arg(Box::new(clonable_state)), Arg(Box::new(1))],
                ExecutionSchedule::Every(Duration::from_secs(10)),
                None,
            )
            .unwrap();
    }

    #[test]
    fn memory_executor() {
        let mut executor = MemoryExecutor::new();
        let task = |args: &[Arg]| {
            let arg1 = args[0].0.downcast_ref::<Arc<Arg1>>().unwrap();
            let arg2 = args[1].0.downcast_ref::<Arg2>().unwrap().clone();
            assert_eq!(arg1.a, 1);
            assert_eq!(arg2.a, 2);
            Ok(())
        };

        fn task1(args: &[Arg]) -> Result<()> {
            let arg1 = args[0].0.downcast_ref::<Arc<Arg1>>().unwrap();
            let arg2 = args[1].0.downcast_ref::<Arg2>().unwrap().clone();
            assert_eq!(arg1.a, 10);
            assert_eq!(arg2.a, 20);
            Ok(())
        }

        executor
            .launch(
                task,
                vec![
                    Arg(Box::new(Arc::new(Arg1 { a: 1 }))),
                    Arg(Box::new(Arg2 { a: 2 })),
                ],
                ExecutionSchedule::Once(Duration::from_secs(1), false),
                None,
            )
            .unwrap();

        executor
            .launch(
                task1,
                vec![
                    Arg(Box::new(Arc::new(Arg1 { a: 10 }))),
                    Arg(Box::new(Arg2 { a: 20 })),
                ],
                ExecutionSchedule::Once(Duration::from_secs(2), false),
                None,
            )
            .unwrap();

        executor.join().unwrap();
    }
    #[tokio::test]
    async fn async_memory_executor() {
        let mut executor = AsyncMemoryExecutor::new();
        fn task1(args: &[Arg]) -> BoxPinnedFuture<'_, Result<String>> {
            Box::pin(async {
                let arg1 = args[0].0.downcast_ref::<Arc<Arg1>>().unwrap();
                let arg2 = args[1].0.downcast_ref::<Arg2>().unwrap().clone();
                assert_eq!(arg1.a, 10);
                assert_eq!(arg2.a, 20);
                Ok("yes".to_string())
            })
        }

        executor
            .launch(
                task1,
                vec![
                    Arg(Box::new(Arc::new(Arg1 { a: 10 }))),
                    Arg(Box::new(Arg2 { a: 20 })),
                ],
                ExecutionSchedule::Once(Duration::from_secs(2), false),
                None,
            )
            .await
            .unwrap();

        executor.join().await.unwrap();
    }
}
