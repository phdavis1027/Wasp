//! Minimal example demonstrating the Wasp component server API.

use tokio_xmpp::Component;
use warp::{Filter, ServeComponent};

#[tokio::main]
async fn main() {
    let component = Component::new("sgxbwmsgsv2.localhost", "secret")
        .await
        .expect("Failed to connect");

    println!("Component connected. Running...");

    component
        .serve(
            // Handle IQ stanzas with a fixed reply
            warp::iq()
                .and(warp::reply("Hello, Iq!"))
                // Handle messages by echoing the body back
                .or(warp::message().and(warp::echo()))
                // Handle presence (log and send no reply)
                .or(warp::presence()
                    .and(warp::sink())
                    .with(warp::log("presence"))),
        )
        .run()
        .await;
}
