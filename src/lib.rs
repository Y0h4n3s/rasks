use std::any::Any;
use std::thread::{self, JoinHandle};
use std::time::Duration;
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
}

pub struct Arg(Box<dyn Any + Send + Sync>);

pub trait ExecutionResult {
    type Type;
    fn get(&self) -> Self::Type;
}

pub trait Executor {
    fn launch<T>(&mut self, task: T, args: Vec<Arg>, schedule: ExecutionSchedule)
    where
        T: Fn(&Vec<Arg>) -> Result<()> + Send + Sync + 'static;
    fn join(self) -> Result<()>;
}

#[derive(Default)]
pub struct MemoryExecutor {
    tasks: Vec<JoinHandle<Result<()>>>,
}

impl MemoryExecutor {
    pub fn new() -> Self {
        Self { tasks: vec![] }
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
    fn launch<T>(&mut self, task: T, args: Vec<Arg>, schedule: ExecutionSchedule)
    where
        T: Fn(&Vec<Arg>) -> Result<()> + Send + Sync + 'static,
    {
        self.tasks.push(thread::spawn(move || {
            let mut schedule = schedule;
            while let Ok(_) = schedule.next_tick() {
                task(&args)?;
            }
            Ok::<(), anyhow::Error>(())
        }));
    }

    fn join(self) -> Result<()> {
        for task in self.tasks {
            task.join().unwrap().unwrap()
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::{Arg, ExecutionSchedule, Executor, MemoryExecutor, Result};
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

        let task = |args: &Vec<Arg>| {
            // downcast your args
            if let (Some(arg1), Some(_arg2)) = (
                args[0].0.downcast_ref::<Arc<Mutex<String>>>(),
                args[1].0.downcast_ref::<u32>()) {
                // execute your tasks logic here
                let mut arg = arg1.lock().expect("poisoned mutex");
                *arg = "new".to_string();
            }

            // task closures should return anyhow::Result<()>
            Ok(())
        };

        let clonable_state = Arc::new(Mutex::new("old"));
        executor.launch(
            task,
            vec![
                Arg(Box::new(clonable_state)),
                Arg(Box::new(1))
            ],
            ExecutionSchedule::Every(Duration::from_secs(10))
        );

    }
    #[test]
    fn memory_executor() {
        let mut executor = MemoryExecutor::new();
        let task = |args: &Vec<Arg>| {
            let arg1 = args[0].0.downcast_ref::<Arc<Arg1>>().unwrap();
            let arg2 = args[1].0.downcast_ref::<Arg2>().unwrap().clone();
            assert_eq!(arg1.a, 1);
            assert_eq!(arg2.a, 2);
            Ok(())
        };

        fn task1(args: &Vec<Arg>) -> Result<()> {
            let arg1 = args[0].0.downcast_ref::<Arc<Arg1>>().unwrap();
            let arg2 = args[1].0.downcast_ref::<Arg2>().unwrap().clone();
            assert_eq!(arg1.a, 10);
            assert_eq!(arg2.a, 20);
            Ok(())
        }

        executor.launch(
            task,
            vec![
                Arg(Box::new(Arc::new(Arg1 { a: 1 }))),
                Arg(Box::new(Arg2 { a: 2 })),
            ],
            ExecutionSchedule::Once(Duration::from_secs(1), false),
        );

        executor.launch(
            task1,
            vec![
                Arg(Box::new(Arc::new(Arg1 { a: 10 }))),
                Arg(Box::new(Arg2 { a: 20 })),
            ],
            ExecutionSchedule::Once(Duration::from_secs(2), false),
        );

        executor.join().unwrap();
    }
}
