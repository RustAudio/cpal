#[macro_export]
macro_rules! impl_callback {
    (
        $t:tt => $r:ty,
        $name:ident,
        $( $k:ident : $v:ty ),*
    ) => {
        pub(super) struct $name {
            callback: Arc<Mutex<Box<dyn $t($($v),*) -> $r + Sync + Send + 'static>>>
        }

        impl <F> From<F> for $name
        where
            F: $t($($v),*) -> $r + Sync + Send + 'static
        {
            fn from(value: F) -> Self {
                Self { callback: Arc::new(Mutex::new(Box::new(value))) }
            }
        }

        impl $name
        {
            pub fn call(&self, $($k: $v),*) -> $r
            {
                let callback = self.callback.lock().unwrap();
                callback($($k),*)
            }
        }

        impl Debug for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("$name").finish()
            }
        }

        impl Clone for $name {
            fn clone(&self) -> Self {
                Self { callback: self.callback.clone() }
            }
        }
    }
}

#[macro_export]
macro_rules! impl_callback_generic {
    (
        $t:tt => $r:ty,
        $name:ident<$( $p:ident $(: $clt:lifetime)? ),*>,
        $($k:ident : $v:ty),*
    ) => {
        pub(super) struct $name<$($p $(: $clt )? ),*> {
            callback: Arc<Mutex<Box<dyn $t($($v),*) -> $r + Sync + Send + 'static>>>
        }

        impl <$($p $(: $clt )? ),*, F> From<F> for $name<$($p),*>
        where
            F: $t($($v),*) -> $r + Sync + Send + 'static
        {
            fn from(value: F) -> Self {
                Self { callback: Arc::new(Mutex::new(Box::new(value))) }
            }
        }

        impl <$($p $(: $clt )? ),*> $name<$($p),*>
        {
            pub fn call(&self, $($k: $v),*) -> $r
            {
                let callback = self.callback.lock().unwrap();
                callback($($k),*)
            }
        }

        impl <$($p $(: $clt )? ),*> Debug for $name<$($p),*> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("$name").finish()
            }
        }

        impl <$($p $(: $clt )? ),*> Clone for $name<$($p),*> {
            fn clone(&self) -> Self {
                Self { callback: self.callback.clone() }
            }
        }
    }
}