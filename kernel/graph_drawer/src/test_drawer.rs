use super::{Graph, Point, draw_graph};

pub fn test_cursor(_: Option<u64>) {
    
}

pub fn test_draw(_: Option<u64>) {
    let point = Graph::Point(Point{x:300, y:200});
    draw_graph(point, 10, 0x00FFFF).unwrap();

    let point = Graph::Point(Point{x:300, y:200});
    draw_graph(point, 20, 0xFFFFFF).unwrap();
}