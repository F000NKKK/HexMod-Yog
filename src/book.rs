//! thehexbook (yog-book). Every text is a translation key of
//! `resources/Book.yres` (English, `Book.ru_ru.yres` Russian): the book is
//! registered with `i18n`, so it's shown in the game's language.
//!
//! The "Getting Started" prose is ported from the original Hex Casting
//! book (Patchouli markup flattened to plain text, split into short pages:
//! yog-book doesn't paginate). Original content (c) gamma-delta /
//! FallingColors contributors, MIT-licensed -- see LICENSE.

use yog_api::book::{Book as YogBook, BookCategory, BookEntry, BookPage};

use crate::content::Items;
use crate::res::Book;

const CATEGORY_BASICS: &str = "hexcasting:basics";
const CATEGORY_PATTERNS: &str = "hexcasting:patterns";

fn page(key: &str) -> BookPage {
    BookPage::Text {
        text: key.into(),
        title: None,
    }
}

pub fn register_book() -> YogBook {
    YogBook::new(Items::THEHEXBOOK, Book::BOOK_NAME_KEY)
        .author("F000NK, Yog Team")
        .i18n(true)
        .add_category(BookCategory {
            id: CATEGORY_BASICS.into(),
            name: Book::BOOK_CATEGORY_BASICS_KEY.into(),
            description: Some(Book::BOOK_CATEGORY_BASICS_DESC_KEY.into()),
            sortnum: 0,
            icon: Some(Items::STAFF_OAK.into()),
            icon_svg: None,
        })
        .add_category(BookCategory {
            id: CATEGORY_PATTERNS.into(),
            name: Book::BOOK_CATEGORY_PATTERNS_KEY.into(),
            description: Some(Book::BOOK_CATEGORY_PATTERNS_DESC_KEY.into()),
            sortnum: 1,
            icon: Some(Items::TOME.into()),
            icon_svg: None,
        })
        .add_entry(BookEntry {
            id: "hexcasting:media".into(),
            name: Book::BOOK_ENTRY_MEDIA_KEY.into(),
            category: CATEGORY_BASICS.into(),
            icon: Some(Items::AMETHYST_DUST.into()),
            priority: 0,
            read_by_default: true,
            pages: vec![
                page(Book::BOOK_ENTRY_MEDIA_PAGE1_KEY),
                page(Book::BOOK_ENTRY_MEDIA_PAGE2_KEY),
                page(Book::BOOK_ENTRY_MEDIA_PAGE3_KEY),
                page(Book::BOOK_ENTRY_MEDIA_PAGE4_KEY),
                page(Book::BOOK_ENTRY_MEDIA_PAGE5_KEY),
            ],
            ..Default::default()
        })
        .add_entry(BookEntry {
            id: "hexcasting:geodes".into(),
            name: Book::BOOK_ENTRY_GEODES_KEY.into(),
            category: CATEGORY_BASICS.into(),
            icon: Some("minecraft:amethyst_block".into()),
            priority: 1,
            read_by_default: true,
            pages: vec![
                page(Book::BOOK_ENTRY_GEODES_PAGE1_KEY),
                page(Book::BOOK_ENTRY_GEODES_PAGE2_KEY),
                page(Book::BOOK_ENTRY_GEODES_PAGE3_KEY),
                page(Book::BOOK_ENTRY_GEODES_PAGE4_KEY),
                page(Book::BOOK_ENTRY_GEODES_PAGE5_KEY),
                page(Book::BOOK_ENTRY_GEODES_PAGE6_KEY),
                page(Book::BOOK_ENTRY_GEODES_PAGE7_KEY),
            ],
            ..Default::default()
        })
        .add_entry(BookEntry {
            id: "hexcasting:couldnt_cast".into(),
            name: Book::BOOK_ENTRY_COULDNT_CAST_KEY.into(),
            category: CATEGORY_BASICS.into(),
            icon: Some("minecraft:textures/mob_effect/nausea.png".into()),
            priority: 2,
            read_by_default: true,
            pages: vec![
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE1_KEY),
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE2_KEY),
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE3_KEY),
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE4_KEY),
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE5_KEY),
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE6_KEY),
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE7_KEY),
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE8_KEY),
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE9_KEY),
                page(Book::BOOK_ENTRY_COULDNT_CAST_PAGE10_KEY),
            ],
            ..Default::default()
        })
        .add_entry(BookEntry {
            id: "hexcasting:start_to_see".into(),
            name: Book::BOOK_ENTRY_START_TO_SEE_KEY.into(),
            category: CATEGORY_BASICS.into(),
            icon: Some("minecraft:textures/mob_effect/blindness.png".into()),
            priority: 3,
            read_by_default: true,
            pages: vec![
                page(Book::BOOK_ENTRY_START_TO_SEE_PAGE1_KEY),
                page(Book::BOOK_ENTRY_START_TO_SEE_PAGE2_KEY),
                page(Book::BOOK_ENTRY_START_TO_SEE_PAGE3_KEY),
                page(Book::BOOK_ENTRY_START_TO_SEE_PAGE4_KEY),
                page(Book::BOOK_ENTRY_START_TO_SEE_PAGE5_KEY),
                page(Book::BOOK_ENTRY_START_TO_SEE_PAGE6_KEY),
                page(Book::BOOK_ENTRY_START_TO_SEE_PAGE7_KEY),
            ],
            ..Default::default()
        })
        .add_entry(BookEntry {
            id: "hexcasting:patterns_intro".into(),
            name: Book::BOOK_ENTRY_PATTERNS_INTRO_KEY.into(),
            category: CATEGORY_PATTERNS.into(),
            icon: None,
            priority: 0,
            read_by_default: true,
            pages: vec![
                page(Book::BOOK_ENTRY_PATTERNS_INTRO_PAGE1_KEY),
                page(Book::BOOK_ENTRY_PATTERNS_INTRO_PAGE2_KEY),
            ],
            ..Default::default()
        })
}
