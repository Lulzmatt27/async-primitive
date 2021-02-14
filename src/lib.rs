use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use futures::task::AtomicWaker;

pub struct Oneshot<T> {
    message: UnsafeCell<MaybeUninit<T>>,
    waker: AtomicWaker,
    has_message: AtomicBool,
}

pub struct Sender<'a, T>(&'a Oneshot<T>);
pub struct Receiver<'a, T>(&'a Oneshot<T>);

impl<T> Oneshot<T> {
    pub const fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            waker: AtomicWaker::new(),
            has_message: AtomicBool::new(false),
        }
    }

    /// NOTE(unsafe): This function must not be used when the oneshot might
    /// contain a message or a `Sender` exists referencing this oneshot. This
    /// means it can't be used concurrently with itself or the latter to run
    /// will violate that constraint.
    pub unsafe fn put(&self, message: T) {
        self.message.get().write(MaybeUninit::new(message));
        self.has_message.store(true, Ordering::Release);
        self.waker.wake();
    }

    /// NOTE(unsafe): This function must not be used concurrently with itself,
    /// but it can be used concurrently with put.
    pub unsafe fn take(&self) -> Option<T> {
        if self.has_message.load(Ordering::Acquire) {
            let message = self.message.get().read().assume_init();
            self.has_message.store(false, Ordering::Release);
            Some(message)
        } else {
            None
        }
    }

    /// Split the Oneshot into a Sender and a Receiver. The Sender can send one
    /// message. The Receiver is a Future and can be used to await that message.
    /// If the Receiver is dropped before taking the message, the message is
    /// dropped as well. This function can be called again after the lifetimes
    /// of the Sender and Receiver end in order to send a new message.
    pub fn split<'a>(&'a mut self) -> (Sender<'a, T>, Receiver<'a, T>) {
        unsafe { self.take() };
        (Sender(self), Receiver(self))
    }

    /// NOTE(unsafe): There must be no more than one `Receiver` at a time.
    /// `take` should not be called while a `Receiver` exists.
    pub unsafe fn recv<'a>(&'a self) -> Receiver<'a, T> {
        Receiver(self)
    }

    /// NOTE(unsafe): There must be no more than one `Sender` at a time. `put`
    /// should not be called while a `Sender` exists. The `Oneshot` must be
    /// empty when the `Sender` is created.
    pub unsafe fn send<'a>(&'a self) -> Sender<'a, T> {
        Sender(self)
    }

    pub fn is_empty(&self) -> bool {
        !self.has_message.load(Ordering::AcqRel)
    }
}

impl<T> Drop for Oneshot<T> {
    fn drop(&mut self) {
        unsafe { self.take() };
    }
}

impl <'a, T> Sender<'a, T>{
    pub fn send(self, message: T){
        unsafe {self.0.put(message)};
    }
}

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

impl<'a, T> Future for Receiver<'a, T>{
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        self.0.waker.register(cx.waker());
        if let Some(message) = unsafe { self.0.take()}{
            Poll::Ready(message)
        }
        else{
            Poll::Pending
        }
    }
}

impl<'a, T> Drop for Receiver<'a, T>{
    fn drop(&mut self){
        unsafe{ self.0.take()};
    }
}