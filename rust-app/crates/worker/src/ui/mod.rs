mod api;
pub(crate) mod controls;
mod main;
mod model;
mod motion;
mod native;
pub(crate) mod runtime;
mod shape;

pub(crate) use model::{ApiEditor, ApiWindow, HistoryItem, MainWindow};
pub(crate) use native::WindowState;
pub(crate) use runtime::{Timer, invoke_from_event_loop, quit_event_loop};

use native::WindowState as NativeWindow;
use std::{
    any::Any,
    cell::RefCell,
    collections::HashMap,
    marker::PhantomData,
    rc::{Rc, Weak as LocalWeak},
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Rows<T>(Rc<Vec<T>>);
impl<T> Default for Rows<T> {
    fn default() -> Self {
        Self(Rc::new(Vec::new()))
    }
}
impl<T> Rows<T> {
    pub fn new(rows: Vec<T>) -> Self {
        Self(Rc::new(rows))
    }
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }
    pub fn row_count(&self) -> usize {
        self.0.len()
    }
    #[cfg(test)]
    pub fn row_data(&self, index: usize) -> Option<T>
    where
        T: Clone,
    {
        self.0.get(index).cloned()
    }
}

thread_local! {
    static UI_HANDLES: RefCell<HashMap<usize, LocalWeak<dyn Any>>> = RefCell::new(HashMap::new());
}
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

/// Worker threads carry the identity; only the UI thread can resolve it.
pub(crate) struct Weak<T>(usize, PhantomData<fn() -> T>);
impl<T> Clone for Weak<T> {
    fn clone(&self) -> Self {
        Self(self.0, PhantomData)
    }
}
impl<T> Default for Weak<T> {
    fn default() -> Self {
        Self(0, PhantomData)
    }
}

struct Component<S, A> {
    id: usize,
    state: RefCell<S>,
    actions: A,
    window: NativeWindow,
}
impl<S, A> Drop for Component<S, A> {
    fn drop(&mut self) {
        let _ = UI_HANDLES.try_with(|handles| handles.borrow_mut().remove(&self.id));
    }
}

macro_rules! component {
    (
        $name:ident, $state:ident, $actions:ident, $view:ident;
        properties { $( $prop:ident : $type:ty = $default:expr, )* }
        actions { $( $event:ident ( $( $arg:ident : $arg_type:ty ),* ); )* }
    ) => {
        #[derive(Clone)]
        pub(crate) struct $state { $(pub $prop: $type,)* }
        impl Default for $state {
            fn default() -> Self { Self { $($prop: $default,)* } }
        }

        #[derive(Default)]
        struct $actions {
            $( $event: std::cell::RefCell<Option<std::rc::Rc<dyn Fn($($arg_type),*)>>>, )*
        }

        #[derive(Clone)]
        pub(crate) struct $name(std::rc::Rc<Component<$state, $actions>>);

        impl super::Weak<$name> {
            pub fn upgrade(&self) -> Option<$name> {
                super::UI_HANDLES.with_borrow(|handles| {
                    let handle = handles.get(&self.0)?.upgrade()?;
                    handle.downcast().ok().map($name)
                })
            }
        }

        #[allow(dead_code)]
        impl $name {
            pub fn new() -> std::io::Result<Self> {
                let id = super::NEXT_ID.fetch_add(1, super::Ordering::Relaxed);
                let inner = std::rc::Rc::new(Component {
                    id,
                    state: std::cell::RefCell::new($state::default()),
                    actions: $actions::default(),
                    window: WindowState::default(),
                });
                let erased: std::rc::Rc<dyn std::any::Any> = inner.clone();
                super::UI_HANDLES.with_borrow_mut(|handles| {
                    handles.insert(id, std::rc::Rc::downgrade(&erased));
                });
                Ok(Self(inner))
            }
            pub fn as_weak(&self) -> super::Weak<Self> {
                super::Weak(self.0.id, std::marker::PhantomData)
            }
            pub fn window(&self) -> &WindowState { &self.0.window }
            pub fn state(&self) -> $state { self.0.state.borrow().clone() }
            pub fn refresh(&self) { self.0.window.refresh(); }

            paste::paste! {
                $(
                    pub fn [<get_ $prop>](&self) -> $type { self.0.state.borrow().$prop.clone() }
                    pub fn [<set_ $prop>](&self, value: $type) {
                        let mut state = self.0.state.borrow_mut();
                        if state.$prop != value {
                            state.$prop = value;
                            drop(state);
                            self.refresh();
                        }
                    }
                )*
                $(
                    pub fn [<on_ $event>](&self, callback: impl Fn($($arg_type),*) + 'static) {
                        *self.0.actions.$event.borrow_mut() = Some(std::rc::Rc::new(callback));
                    }
                    pub fn [<invoke_ $event>](&self, $($arg: $arg_type),*) {
                        let callback = self.0.actions.$event.borrow().clone();
                        if let Some(callback) = callback { callback($($arg),*); }
                    }
                )*
            }
        }
    };
}
use component;
