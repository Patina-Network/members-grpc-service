use crate::db;
use std::{sync::Arc, time::SystemTime};
use tonic::{Code, Request, Response, Result, Status};

tonic::include_proto!("helloworld");
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("descriptor");

// main will need this
pub use greeter_service_server::GreeterServiceServer;

#[derive(Debug, Default)]
pub struct GreeterService {
    greetings_repo: Arc<db::GreetingsRepository>,
}

impl GreeterService {
    pub fn new(greetings_repo: Arc<db::GreetingsRepository>) -> Self {
        Self { greetings_repo }
    }
}

fn greeting_response_from_greeting(
    recipient_name: String,
    greeting: db::Greeting,
) -> GreetingResponse {
    let db::Greeting {
        id,
        sender_name,
        message,
        timestamp,
    } = greeting;
    let std_time: SystemTime = timestamp.into();
    let response_message = format!("{sender_name} says {message}");

    GreetingResponse {
        id,
        message: response_message,
        sender_name,
        recipient_name,
        received_at: Some(std_time.into()),
    }
}

#[tonic::async_trait]
impl greeter_service_server::GreeterService for GreeterService {
    async fn echo_hello(
        &self,
        request: Request<EchoHelloRequest>,
    ) -> Result<Response<EchoHelloResponse>, Status> {
        let request = request.into_inner();
        let name = request.name;

        if name.is_empty() {
            return Err(Status::new(
                Code::InvalidArgument,
                "please pass in a non-empty string for name",
            ));
        }

        Ok(EchoHelloResponse {
            response: format!("server says hello, {name}!"),
        }
        .into())
    }

    async fn say_greeting(
        &self,
        request: Request<SayGreetingRequest>,
    ) -> Result<Response<SayGreetingResponse>, Status> {
        let request = request.into_inner();

        let SayGreetingRequest {
            recipient_name,
            sender_name,
            greeting,
        } = request;

        self.greetings_repo
            .add_new_greeting(db::NewGreeting {
                recipient_name,
                sender_name,
                message: greeting,
            })
            .await;

        Ok(SayGreetingResponse {}.into())
    }

    async fn get_greetings_by_name(
        &self,
        request: Request<GetGreetingsByNameRequest>,
    ) -> Result<Response<GreetingsResponse>, Status> {
        let request = request.into_inner();

        match request.recipient_name.as_deref() {
            Some("") | None => {
                let replies = self
                    .greetings_repo
                    .get_all_greetings()
                    .await
                    .into_iter()
                    .map(|(recipient_name, greeting)| {
                        greeting_response_from_greeting(recipient_name, greeting)
                    })
                    .collect();

                Ok(GreetingsResponse { replies }.into())
            }
            Some(n) => match self.greetings_repo.get_greetings_by_name(n).await {
                Some(greetings) => {
                    let replies = greetings
                        .into_iter()
                        .map(|greeting| greeting_response_from_greeting(n.to_string(), greeting))
                        .collect();

                    Ok(GreetingsResponse { replies }.into())
                }
                None => Err(Status::not_found("no greetings found for this name")),
            },
        }
    }
}
