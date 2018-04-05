use super::{Graph, Point, Line, Square, draw_graph};

pub fn test_cursor(_: Option<u64>) {
    
}

pub fn test_draw(_: Option<u64>) {
    let point = Graph::Point(Point{x:300, y:200});
    draw_graph(point, 10, 0x00FFFF).unwrap();

    let point = Graph::Point(Point{x:300, y:200});
    draw_graph(point, 20, 0xFFFFFF).unwrap();

    let line = Graph::Line(Line{start_x:20, start_y:30, end_x:200, end_y:300});
    draw_graph(line, 17, 0xFF0000).unwrap();


    let square = Graph::Square(Square{x:30, y:50, width:40, height:30, fill:false});
    draw_graph(square, 19, 0x3768FF).unwrap();
    
    let line = Graph::Line(Line{start_x:20, start_y:50, end_x:500, end_y:50});
    draw_graph(line, 19, 0x00FF00).unwrap();

    let line = Graph::Line(Line{start_x:20, start_y:300, end_x:300, end_y:50});
    draw_graph(line, 19, 0x0000FF).unwrap();


    let square = Graph::Square(Square{x:40, y:60, width:100, height:30, fill:true});
    draw_graph(square, 20, 0xFF3769).unwrap();

}