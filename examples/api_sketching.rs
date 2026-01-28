//! Minimal example demonstrating the Wasp component server API.

use tokio_xmpp::Component;
use warp::{Filter, ServeComponent};

#[tokio::main]
async fn main() {
    let component = Component::new("sgxbwmsgsv2.localhost", "secret")
        .await
        .expect("Failed to connect");

    // component.serve(warp::reply("Hello, World!")).run().await;
    component.serve(warp::iq()).run().await;

    component.serve(warp::iq::param()).await;
}
