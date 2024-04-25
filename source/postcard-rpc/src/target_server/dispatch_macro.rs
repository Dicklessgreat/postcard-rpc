/// # Define Dispatch Macro
///
/// ```rust
/// # use postcard_rpc::target_server::dispatch_macro::fake::*;
/// # use postcard_rpc::{endpoint, target_server::Sender, WireHeader, define_dispatch};
/// # use postcard::experimental::schema::Schema;
/// # use embassy_usb_driver::{Bus, ControlPipe, EndpointIn, EndpointOut};
/// # use serde::{Deserialize, Serialize};
/// define_dispatch! {
///     dispatcher: Dispatcher<Mutex = FakeMutex, Driver = FakeDriver>;
///     AlphaEndpoint => async alpha_handler,
///     BetaEndpoint => async beta_handler,
///     GammaEndpoint => async gamma_handler,
///     DeltaEndpoint => blocking delta_handler,
///     EpsilonEndpoint => spawn epsilon_handler_task,
/// }
///
/// async fn alpha_handler(_header: WireHeader, _body: AReq) -> AResp {
///     todo!()
/// }
///
/// async fn beta_handler(_header: WireHeader, _body: BReq) -> BResp {
///     todo!()
/// }
///
/// async fn gamma_handler(_header: WireHeader, _body: GReq) -> GResp {
///     todo!()
/// }
///
/// fn delta_handler(_header: WireHeader, _body: DReq) -> DResp {
///     todo!()
/// }
///
/// #[embassy_executor::task]
/// async fn epsilon_handler_task(_header: WireHeader, _body: EReq, _sender: Sender<FakeMutex, FakeDriver>) {
///     todo!()
/// }
/// ```

#[macro_export]
macro_rules! define_dispatch {
    // This is the "blocking execution" arm for defining an endpoint
    (@arm blocking ($endpoint:ty) $handler:ident $header:ident $req:ident $sender:ident) => {
        {
            $crate::standard_icd::Outcome::Reply($handler($header.clone(), $req))
        }
    };
    // This is the "async execution" arm for defining an endpoint
    (@arm async ($endpoint:ty) $handler:ident $header:ident $req:ident $sender:ident) => {
        {
            $crate::standard_icd::Outcome::Reply($handler($header.clone(), $req).await)
        }
    };
    // This is the "spawn an embassy task" arm for defining an endpoint
    (@arm spawn ($endpoint:ty) $handler:ident $header:ident $req:ident $sender:ident) => {
        {
            let spawner = ::embassy_executor::Spawner::for_current_executor().await;
            if spawner.spawn($handler($header.clone(), $req, $sender.clone())).is_ok() {
                $crate::standard_icd::Outcome::SpawnSuccess
            } else {
                $crate::standard_icd::Outcome::SpawnFailure
            }
        }
    };
    // This is the main entrypoint
    (
        dispatcher: $name:ident<Mutex = $mutex:ty, Driver = $driver:ty>;
        $($endpoint:ty => $flavor:tt $handler:ident,)*
    ) => {
        /// This is a structure that handles dispatching, generated by the
        /// `postcard-rpc::define_dispatch!()` macro.
        pub struct $name {
            pub sender: $crate::target_server::Sender<$mutex, $driver>
        }

        impl $name {
            /// Create a new instance of the dispatcher
            pub fn new(
                tx_buf: &'static mut [u8],
                ep_in: <$driver as ::embassy_usb::driver::Driver<'static>>::EndpointIn,
            ) -> Self {
                static SENDER_INNER: ::static_cell::StaticCell<
                    ::embassy_sync::mutex::Mutex<$mutex, $crate::target_server::SenderInner<$driver>>,
                > = ::static_cell::StaticCell::new();
                $name {
                    sender: Sender::init_sender(&SENDER_INNER, tx_buf, ep_in),
                }
            }
        }

        impl $crate::target_server::Dispatch for $name {
            type Mutex = $mutex;
            type Driver = $driver;

            async fn dispatch(
                &self,
                hdr: $crate::WireHeader,
                body: &[u8],
                sender: $crate::target_server::Sender<Self::Mutex, Self::Driver>,
            ) {
                // Unreachable patterns lets us know if we had any duplicated request keys.
                // If you hit this error: you either defined the same endpoint twice, OR you've
                // had a schema collision.
                #[deny(unreachable_patterns)]
                match hdr.key {
                    $(
                        <$endpoint as $crate::Endpoint>::REQ_KEY => {
                            let Ok(req) = postcard::from_bytes::<<$endpoint as $crate::Endpoint>::Request>(body) else {
                                let err = $crate::standard_icd::WireError::DeserFailed;
                                self.error(hdr.seq_no, err, sender).await;
                                return;
                            };
                            use $crate::standard_icd::Outcome;

                            let resp: Outcome<<$endpoint as $crate::Endpoint>::Response> = define_dispatch!(@arm $flavor ($endpoint) $handler hdr req sender);
                            match resp {
                                Outcome::Reply(t) => {
                                    if sender.reply::<$endpoint>(hdr.seq_no, &t).await.is_err() {
                                        let err = $crate::standard_icd::WireError::SerFailed;
                                        self.error(hdr.seq_no, err, sender).await;
                                        return;
                                    }
                                }
                                Outcome::SpawnSuccess => {},
                                Outcome::SpawnFailure => {
                                    let err = $crate::standard_icd::WireError::FailedToSpawn;
                                    self.error(hdr.seq_no, err, sender).await;
                                }
                            }
                        }
                    )*
                    other => {
                        let err = $crate::standard_icd::WireError::UnknownKey(other.to_bytes());
                        self.error(hdr.seq_no, err, sender).await;
                        return;
                    },
                }
            }
            async fn error(
                &self,
                seq_no: u32,
                error: $crate::standard_icd::WireError,
                sender: $crate::target_server::Sender<Self::Mutex, Self::Driver>,
            ) {
                let _ = sender.reply_keyed(seq_no, $crate::standard_icd::ERROR_KEY, &error).await;
            }

            fn sender(&self) -> $crate::target_server::Sender<Self::Mutex, Self::Driver> {
                self.sender.clone()
            }
        }

    }
}

/// This is a basic example that everything compiles. It is intended to exercise the macro above,
/// as well as provide impls for docs. Don't rely on any of this!
#[doc(hidden)]
#[allow(dead_code)]
pub mod fake {
    #[allow(unused_imports)]
    use crate::{endpoint, target_server::Sender, Schema, WireHeader};
    use embassy_usb_driver::{Bus, ControlPipe, EndpointIn, EndpointOut};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Schema)]
    pub struct AReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct AResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct BReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct BResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct GReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct GResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct DReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct DResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct EReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct EResp;

    endpoint!(AlphaEndpoint, AReq, AResp, "alpha");
    endpoint!(BetaEndpoint, BReq, BResp, "beta");
    endpoint!(GammaEndpoint, GReq, GResp, "gamma");
    endpoint!(DeltaEndpoint, DReq, DResp, "delta");
    endpoint!(EpsilonEndpoint, EReq, EResp, "epsilon");

    pub struct FakeMutex;
    pub struct FakeDriver;
    pub struct FakeEpOut;
    pub struct FakeEpIn;
    pub struct FakeCtlPipe;
    pub struct FakeBus;

    impl embassy_usb_driver::Endpoint for FakeEpOut {
        fn info(&self) -> &embassy_usb_driver::EndpointInfo {
            todo!()
        }

        async fn wait_enabled(&mut self) {
            todo!()
        }
    }

    impl EndpointOut for FakeEpOut {
        async fn read(
            &mut self,
            _buf: &mut [u8],
        ) -> Result<usize, embassy_usb_driver::EndpointError> {
            todo!()
        }
    }

    impl embassy_usb_driver::Endpoint for FakeEpIn {
        fn info(&self) -> &embassy_usb_driver::EndpointInfo {
            todo!()
        }

        async fn wait_enabled(&mut self) {
            todo!()
        }
    }

    impl EndpointIn for FakeEpIn {
        async fn write(&mut self, _buf: &[u8]) -> Result<(), embassy_usb_driver::EndpointError> {
            todo!()
        }
    }

    impl ControlPipe for FakeCtlPipe {
        fn max_packet_size(&self) -> usize {
            todo!()
        }

        async fn setup(&mut self) -> [u8; 8] {
            todo!()
        }

        async fn data_out(
            &mut self,
            _buf: &mut [u8],
            _first: bool,
            _last: bool,
        ) -> Result<usize, embassy_usb_driver::EndpointError> {
            todo!()
        }

        async fn data_in(
            &mut self,
            _data: &[u8],
            _first: bool,
            _last: bool,
        ) -> Result<(), embassy_usb_driver::EndpointError> {
            todo!()
        }

        async fn accept(&mut self) {
            todo!()
        }

        async fn reject(&mut self) {
            todo!()
        }

        async fn accept_set_address(&mut self, _addr: u8) {
            todo!()
        }
    }

    impl Bus for FakeBus {
        async fn enable(&mut self) {
            todo!()
        }

        async fn disable(&mut self) {
            todo!()
        }

        async fn poll(&mut self) -> embassy_usb_driver::Event {
            todo!()
        }

        fn endpoint_set_enabled(
            &mut self,
            _ep_addr: embassy_usb_driver::EndpointAddress,
            _enabled: bool,
        ) {
            todo!()
        }

        fn endpoint_set_stalled(
            &mut self,
            _ep_addr: embassy_usb_driver::EndpointAddress,
            _stalled: bool,
        ) {
            todo!()
        }

        fn endpoint_is_stalled(&mut self, _ep_addr: embassy_usb_driver::EndpointAddress) -> bool {
            todo!()
        }

        async fn remote_wakeup(&mut self) -> Result<(), embassy_usb_driver::Unsupported> {
            todo!()
        }
    }

    impl embassy_usb_driver::Driver<'static> for FakeDriver {
        type EndpointOut = FakeEpOut;

        type EndpointIn = FakeEpIn;

        type ControlPipe = FakeCtlPipe;

        type Bus = FakeBus;

        fn alloc_endpoint_out(
            &mut self,
            _ep_type: embassy_usb_driver::EndpointType,
            _max_packet_size: u16,
            _interval_ms: u8,
        ) -> Result<Self::EndpointOut, embassy_usb_driver::EndpointAllocError> {
            todo!()
        }

        fn alloc_endpoint_in(
            &mut self,
            _ep_type: embassy_usb_driver::EndpointType,
            _max_packet_size: u16,
            _interval_ms: u8,
        ) -> Result<Self::EndpointIn, embassy_usb_driver::EndpointAllocError> {
            todo!()
        }

        fn start(self, _control_max_packet_size: u16) -> (Self::Bus, Self::ControlPipe) {
            todo!()
        }
    }

    unsafe impl embassy_sync::blocking_mutex::raw::RawMutex for FakeMutex {
        const INIT: Self = Self;

        fn lock<R>(&self, _f: impl FnOnce() -> R) -> R {
            todo!()
        }
    }

    define_dispatch! {
        dispatcher: TestDispatcher<Mutex = FakeMutex, Driver = FakeDriver>;
        AlphaEndpoint => async test_alpha_handler,
        BetaEndpoint => async test_beta_handler,
        GammaEndpoint => async test_gamma_handler,
        DeltaEndpoint => blocking test_delta_handler,
        // EpsilonEndpoint => spawn test_epsilon_handler_task,
    }

    async fn test_alpha_handler(_header: WireHeader, _body: AReq) -> AResp {
        todo!()
    }

    async fn test_beta_handler(_header: WireHeader, _body: BReq) -> BResp {
        todo!()
    }

    async fn test_gamma_handler(_header: WireHeader, _body: GReq) -> GResp {
        todo!()
    }

    fn test_delta_handler(_header: WireHeader, _body: DReq) -> DResp {
        todo!()
    }

    // #[embassy_executor::task]
    // async fn test_epsilon_handler_task(_header: WireHeader, _body: EReq, _sender: Sender<FakeMutex, FakeDriver>) {
    //     todo!()
    // }
}