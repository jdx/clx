pub mod progress;

use std::{io::Read, sync::Arc, sync::Mutex};

use console::{Key, Term};
use tokio::sync::{RwLock};

pub enum Message<T> {
    Stdin(Key),
    Data(T),
    Stop,
}

trait Model<T>: Send + Sync {
    fn init(&mut self) -> Option<Message<T>>;
    fn update(&mut self, msg: Message<T>) -> Option<Message<T>>;
    fn view(&self) -> String;
}

pub struct Component<M: Model<T>, T: Send + Sync> {
    model: RwLock<M>,
    stdin: bool,
    term: Term,
    height: Mutex<usize>,
    _marker: std::marker::PhantomData<T>,
}

impl<M: Model<T>, T: Send + Sync> Component<M, T> {
    pub fn new(model: M) -> Self {
        Self {
            model: RwLock::new(model),
            stdin: false,
            term: Term::stderr(),
            height: Mutex::new(0),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn stdin(&mut self, stdin: bool) -> &mut Self {
        self.stdin = stdin;
        unimplemented!("stdin is not supported yet");
        // self
    }

    pub fn term(&mut self, term: Term) -> &mut Self {
        self.term = term;
        self
    }

    pub async fn run(self) {
        let this = Arc::new(self);
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);

        let tx = tx.clone();
        let this = this.clone();
        if let Some(msg) = this.model.write().await.init() {
            let _ = tx.send(msg).await;
        }

        // if this.stdin {
        //     let mut rx = read_stdin();
        //     let tx = tx.clone();
        //     tokio::spawn(async move {
        //         while let Ok(k) = rx.recv().await {
        //             if let Err(_) = tx.send(Message::Stdin(k)).await {
        //                 break;
        //             }
        //         }
        //     });
        // }

        // Main update/render loop
        loop {
            this.clear();
            let msg = this.model.read().await.view();
            *this.height.lock().unwrap() = msg.lines().count();
            let _ = this.term.write_line(&msg);
            if let Some(msg) = rx.recv().await {
                if let Message::Stop = msg {
                    break;
                }
                if let Some(msg) = this.model.write().await.update(msg) {
                    let _ = tx.send(msg).await;
                }
            } else {
                unreachable!()
            }
        }
    }

    fn clear(&self) {
        let mut height = self.height.lock().unwrap();
        if *height > 0 {
            let _ = self.term.clear_last_lines(*height);
            *height = 0;
        }
    }

    pub fn height(&self) -> usize {
        *self.height.lock().unwrap()
    }
}

// fn read_stdin() -> broadcast::Receiver<Key> {
//     static ONCE: std::sync::Once = std::sync::Once::new();
//     static mut TX: Option<broadcast::Sender<Key>> = None;

//     unsafe {
//         ONCE.call_once(|| {
//             let (tx, _) = broadcast::channel(16);
//             TX = Some(tx);
//             let tx = TX.as_ref().unwrap().clone();
//             thread::spawn(move || {
//                 let term = Term::stderr();
//                 while let Ok(k) = term.read_key() {
//                     let _ = tx.send(k);
//                 }
//             });
//         });
//         TX.as_ref().unwrap().subscribe()
//     }
// }
