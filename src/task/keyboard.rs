use crate::{print, println};
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use crossbeam_queue::ArrayQueue;
use futures_util::{
    stream::{Stream, StreamExt},
    task::AtomicWaker,
};
use lazy_static::lazy_static;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

/// Queue for holding incoming scancodes from the serial port
// will hold scancodes between the keyboard interrupt, and next
// poll call until it can be handled and registered with the waker

lazy_static! {
    static ref SCANCODE_QUEUE: ArrayQueue<u8> = ArrayQueue::new(100);
    static ref WAKERS: ArrayQueue<AtomicWaker> = ArrayQueue::new(100);
}

// static NEW_SCANCODE_EVENT: LocalManualResetEvent = LocalManualResetEvent::new(false);

/// Called by the keyboard interrupt handler
///
/// Must not block or allocate, as waiting or allocating in a interupt 
/// can lead to memory leaks and laggy execution
pub(crate) fn add_scancode(scancode: u8) {

    // for every waker, push the scancode
    while let Ok(waker) = WAKERS.pop() {
        SCANCODE_QUEUE.push(scancode);
        
        waker.wake();
    }

    // if let Err(_) = SCANCODE_QUEUE.push(scancode) {
    //     println!("WARNING: scancode queue full; dropping keyboard input");
    // } else {
    //     // if it succeeded, wake the waker to alert any functions of a new scancode
    //     println!("add_scancode(): got scancode, waking {:} wakers", WAKERS.len());

    //     while let Ok(waker) = WAKERS.pop() {
    //         waker.wake();
    //     }
    // }
}

/// ScancodeStream structure
// will be used to implement the future_utils stream type,
// which is a simple prototype for something that produces a stream of values
pub struct ScancodeStream {
    _private: (),
}

/// Main Implementation for ScancodeStream
// here we only impement the initialization function
impl ScancodeStream {
    
    pub fn new() -> Self {
        //
        ScancodeStream { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    // Poll the scancode stream 
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        // println!("stream::poll_next() got called");

        // 
        if let Ok(scancode) = SCANCODE_QUEUE.pop() {
            return Poll::Ready(Some(scancode));
        }

        let waker = AtomicWaker::new();
        waker.register(&cx.waker());
        WAKERS.push(waker);

        // println!("stream::poll_next(): registered waker for this stream, total wakers: {:}", WAKERS.len());

        match SCANCODE_QUEUE.pop() {
            Ok(scancode) => {
                if let Ok(last_waker) = WAKERS.pop() {
                    last_waker.take();

                } else {
                    println!("No Wakers to wake");
                }
                Poll::Ready(Some(scancode))
            }
            Err(crossbeam_queue::PopError) => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

pub async fn print_keypresses() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(layouts::Us104Key, ScancodeSet1, HandleControl::Ignore);

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => print!("{}", character),
                    DecodedKey::RawKey(key) => print!("{:?}", key),
                }
            }
        }
    }

}