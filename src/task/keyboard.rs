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
use conquer_once::spin::OnceCell;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};



// OnceCell that holds a queue of scancodes
// we use a OnceCell here to ensure that these only get initialized 
// inside of the ScancodeStream initializer, and not inside the add_scancode function
// which is run a interrupt
static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();

// OnceCell that holds a queue of atomic wakers
static WAKER_QUEUE: OnceCell<ArrayQueue<AtomicWaker>> = OnceCell::uninit();


/// Called by the keyboard interrupt handler
///
/// Must not block or allocate on the heap, as waiting or allocating in a interupt 
/// can lead to deadlocks
pub(crate) fn add_scancode(scancode: u8) {
    // try to get the scancode queue
    let scancode_queue = SCANCODE_QUEUE
        .try_get()
        .expect("scancode queue not initialized");
    // try too get the waker queue
    let waker_queue = WAKER_QUEUE
        .try_get()
        .expect("waker queue not initialized");


    // Here the waker queue should be filled with an atomic waker for each 
    // async context that creates a scancode stream
    // For each of these, we want to signal the context that there is a scancode to be read
    // aditionally, we put a copy of the given scancode into the queue for each context,
    // as they will be called in any order, and each needs a copy of the scancode
    // the easiest way to give each of them a scancode is to just copy it into the queue.
    while let Ok(waker) = waker_queue.pop() {
        // try to push the scancode into the queue
        if let Err(_) = scancode_queue.push(scancode) {
            println!("WARNING: scancode queue full; dropping keyboard input");
        } else {
            // if we pushed the scancode for a given context, wake it up
            waker.wake();
        }
        
    }

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
    
    // Create a new scancode stream
    pub fn new() -> Self {
        // try to initialize both queues, we do not care if it already exists, and if it does there should be low overhead
        SCANCODE_QUEUE.init_once(|| ArrayQueue::new(100));
        WAKER_QUEUE.init_once(|| ArrayQueue::new(100));

        ScancodeStream { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    // Attempt to pull the next value out of the stream 
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        // try to get the scancode queue
        let scancode_queue = SCANCODE_QUEUE
            .try_get()
            .expect("scancode queue not initialized");
        // try to get the waker qeueu
        let waker_queue = WAKER_QUEUE
            .try_get()
            .expect("waker queue not initialized");


        // fast path
        // if we are here, it means we probably got called after we got woken up by the interrupt with a new scancode
        // in that case, we grab it from the queue, and tell the poller that we are ready with some data
        if let Ok(scancode) = scancode_queue.pop() {
            return Poll::Ready(Some(scancode));
        }


        // slow path
        // if not, it means we probably got called by the executor, with no data avaiable
        // so, we create a new AtomicWaker, and register it with the overall context, and put it in the queue
        // this lets us tell store the waker, which will let us wake up the context in the future when we have data
        let waker = AtomicWaker::new();
        waker.register(&cx.waker());
        if let Err(_) = waker_queue.push(waker) {
            println!("WARNING: scancode queue full; dropping keyboard input");
        }
        


        match scancode_queue.pop() {
            Ok(scancode) => {
                if let Ok(last_waker) = waker_queue.pop() {
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