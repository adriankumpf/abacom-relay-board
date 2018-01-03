extern crate abacom_ch341a_relay_board;

fn main() {
    for rb in abacom_ch341a_relay_board::list_relay_boards().unwrap() {
        println!("{:?}", rb);
    }
}
