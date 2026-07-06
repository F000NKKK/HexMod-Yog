//! HexMod in-game book registration (yog-book).

use yog_api::book::{Book, BookCategory, BookEntry, BookPage};

pub fn register_book() -> Book {
    Book::new("hexcasting:thehexbook", "The Hexbook")
        .author("F000NK, Yog Team")
        .add_category(BookCategory {
            id: "hexcasting:basics".into(),
            name: "Basics".into(),
            description: Some("Getting started with Hexcasting.".into()),
            sortnum: 0,
            icon: Some("hexcasting:item/staff".into()),
            icon_svg: None,
        })
        .add_category(BookCategory {
            id: "hexcasting:patterns".into(),
            name: "Patterns".into(),
            description: Some("All about hex patterns.".into()),
            sortnum: 1,
            icon: Some("hexcasting:item/tome".into()),
            icon_svg: None,
        })
        .add_entry(BookEntry {
            id: "hexcasting:first_steps".into(),
            name: "First Steps".into(),
            category: "hexcasting:basics".into(),
            icon: Some("hexcasting:item/thehexbook".into()),
            priority: 1,
            read_by_default: true,
            pages: vec![
                BookPage::Text { text: "Welcome to Hexcasting! ...".into(), title: None },
                BookPage::Text { text: "To start, craft ...".into(), title: None },
            ],
            ..Default::default()
        })
        .add_entry(BookEntry {
            id: "hexcasting:patterns_intro".into(),
            name: "Patterns Introduction".into(),
            category: "hexcasting:patterns".into(),
            pages: vec![
                BookPage::Text { text: "Patterns are the ...".into(), title: None },
                BookPage::Text { text: "Draw patterns ...".into(), title: None },
            ],
            ..Default::default()
        })
}

