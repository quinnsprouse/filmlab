#[cfg(feature = "profile")]
macro_rules! zone {
    ($($span:tt)+) => {
        tracing::info_span!($($span)+).entered()
    };
}

#[cfg(not(feature = "profile"))]
macro_rules! zone {
    ($($span:tt)+) => {
        $crate::profile::Zone
    };
}

#[cfg(feature = "profile")]
macro_rules! note {
    ($($event:tt)+) => {
        tracing::info!($($event)+)
    };
}

#[cfg(not(feature = "profile"))]
macro_rules! note {
    ($($event:tt)+) => {};
}

macro_rules! spent {
    ($name:literal, $work:expr) => {{
        let _zone = zone!($name);
        $work
    }};
}

/// What `zone!` holds when profiling is off: nothing.
#[cfg(not(feature = "profile"))]
pub(crate) struct Zone;
