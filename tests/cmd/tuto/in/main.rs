mod protos {
    buffrs::include!();
}

struct Sensor;

impl protos::sensor_api::sensor_server::Sensor for Sensor {
    fn read_temperature<'life0, 'async_trait>(
        &'life0 self,
        _request: tonic::Request<protos::sensor_api::DeviceId>,
    ) -> ::core::pin::Pin<
        Box<
            dyn ::core::future::Future<
                    Output = std::result::Result<
                        tonic::Response<protos::sensor_api::Measurement>,
                        tonic::Status,
                    >,
                > + ::core::marker::Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        todo!()
    }
}

fn main() {
    todo!()
}
