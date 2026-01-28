use warp::Filter;

fn main() {
    let _ = warp::message();
    let _ = warp::message::param();
    let _ = warp::iq();
    let _ = warp::iq::param();
    let _ = warp::presence();
    let _ = warp::presence::param();
    let _ = warp::id("test");
    let _ = warp::id::param();
}
