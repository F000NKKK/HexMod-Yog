//! HexMod in-game book registration (yog-book).
//!
//! The "Basics" category's flavour text below is ported (translated out of
//! Patchouli's `$(...)`/`_keyword` markup into plain text) from the original
//! Hex Casting book — see `assets/hexcasting/yog_book/` for the source
//! category/entry definitions and `assets/hexcasting/lang/en_us.flatten.json5`
//! for the prose itself. Original content © gamma-delta / FallingColors
//! contributors, MIT-licensed — see LICENSE.

use yog_api::book::{Book, BookCategory, BookEntry, BookPage};

pub fn register_book() -> Book {
    Book::new("hexcasting:thehexbook", "The Hexbook")
        .author("F000NK, Yog Team")
        .add_category(BookCategory {
            id: "hexcasting:basics".into(),
            name: "Getting Started".into(),
            description: Some(
                "The practitioners of this art would cast their so-called Hexes by drawing \
strange patterns in the air with a Staff -- or craft powerful magical items to do the casting \
for them. How might I do the same?".into(),
            ),
            sortnum: 0,
            icon: Some("hexcasting:item/staff".into()),
            icon_svg: None,
        })
        .add_category(BookCategory {
            id: "hexcasting:patterns".into(),
            name: "Patterns".into(),
            description: Some("A list of all the patterns I've discovered, as well as what they do.".into()),
            sortnum: 1,
            icon: Some("hexcasting:item/tome".into()),
            icon_svg: None,
        })
        // Order follows the original book's sortnum: media, geodes, couldnt_cast, start_to_see.
        .add_entry(BookEntry {
            id: "hexcasting:media".into(),
            name: "Media".into(),
            category: "hexcasting:basics".into(),
            icon: Some("hexcasting:amethyst_dust".into()),
            priority: 0,
            read_by_default: true,
            pages: vec![
                BookPage::Text { text: "Media is a form of mental energy external to a mind. All living creatures generate trace amounts of media when thinking about anything; after the thought is finished, the media is released into the environment.\nThe art of casting Hexes is all about manipulating media to do your bidding.".into(), title: None },
                BookPage::Text { text: "Media can exert influences on other media-- the strength and type of influence can be manipulated by drawing media out into patterns.\nScholars of the art used a concentrated blob of media on the end of a stick: by waving it in the air in precise configurations, they were able to manipulate enough media with enough precision to influence the world itself, in the form of a Hex.".into(), title: None },
                BookPage::Text { text: "Sadly, even a fully sentient being (like myself, presumably) can only generate miniscule amounts of media. It would be quite impractical to try and use my own brainpower to cast Hexes.\nBut legend has it that there are underground deposits where media slowly accumulates, growing into crystalline forms.\nIf I could just find one of those...".into(), title: None },
            ],
            ..Default::default()
        })
        .add_entry(BookEntry {
            id: "hexcasting:geodes".into(),
            name: "Geodes".into(),
            category: "hexcasting:basics".into(),
            icon: Some("minecraft:amethyst_block".into()),
            priority: 1,
            read_by_default: true,
            pages: vec![
                BookPage::Text { text: "Aha! While mining deep underground, I found an enormous geode resonating with energy-- energy which pressed against my skull and my thoughts. And now, I hold that pressure in my hand, in solid form. That proves it. This must be the place spoken about in legends where media accumulates.\nThese amethyst crystals must be a convenient, solidified form of Media.".into(), title: None },
                BookPage::Text { text: "It appears that, in addition to the Amethyst Shards I have seen in the past, these crystals can also drop bits of powdered Amethyst Dust, as well as these Charged Amethyst Crystals. It looks like I'll have a better chance of finding the Charged Amethyst Crystals by using a Fortune pickaxe.".into(), title: None },
                BookPage::Text { text: "As I take the beauty of the crystal in, I can feel connections flashing wildly in my mind. It's like the media in the air is entering me, empowering me, elucidating me... It feels wonderful.\nFinally, my study into the arcane is starting to make some sense!\nLet me reread those old legends again, now that I know what I'm looking at.".into(), title: None },
            ],
            ..Default::default()
        })
        .add_entry(BookEntry {
            id: "hexcasting:couldnt_cast".into(),
            name: "A Frustration".into(),
            category: "hexcasting:basics".into(),
            // Original Patchouli icon is a raw texture, not an item — matches
            // 1:1 now that yog-book supports `.png`-suffixed texture icons.
            icon: Some("minecraft:textures/mob_effect/nausea.png".into()),
            priority: 2,
            read_by_default: true,
            pages: vec![
                BookPage::Text { text: "Argh! Why won't it let me cast the spell?!\nThe scroll I found rings with authenticity. I can feel it humming in the scroll-- the pattern is true, or as true as it can be. The spell is right there.\nBut it feels as if it's on the other side of some thin membrane. I called it-- it tried to manifest-- yet it COULD NOT.".into(), title: None },
                BookPage::Text { text: "It felt like the barrier may have weakened ever so slightly from the force that I exerted on the spell; yet despite my greatest efforts-- my deepest focus, my finest amethyst, my precisest drawings-- it refuses to cross the barrier. It's maddening.\nThis is where my arcane studies end? Cursed by impotence, cursed to lose my rightful powers?\nI should take a deep breath. I should meditate on what I have learned, even if it wasn't very much...".into(), title: None },
                BookPage::Text { text: "...After careful reflection... I have discovered a change in myself.\nIt seems... in lieu of amethyst, I've unlocked the ability to cast spells using my own mind and life energy-- just as I read of in the legends of old.\nI'm not sure why I can now. It's just... the truth-knowledge-burden was always there, and I see it now. I know it. I bear it.\nFortunately, I feel my limits as well-- I would get approximately two Charged Amethyst's worth of media out of my health at its prime.".into(), title: None },
                BookPage::Text { text: "I shudder to even consider it-- I've kept my mind mostly intact so far, in my studies. But the fact is-- I form one side of a tenuous link.\nI'm connected to some other side-- a side whose boundary has thinned from that trauma. A place where simple actions spell out eternal glory.\nIs it so wrong, to want it for myself?".into(), title: None },
            ],
            ..Default::default()
        })
        .add_entry(BookEntry {
            id: "hexcasting:start_to_see".into(),
            name: "WHAT DID I SEE".into(),
            category: "hexcasting:basics".into(),
            icon: Some("minecraft:textures/mob_effect/blindness.png".into()),
            priority: 3,
            read_by_default: true,
            pages: vec![
                BookPage::Text { text: "The texts weren't lying. Nature took its due.".into(), title: None },
                BookPage::Text { text: "That... that was...\n...that was one of the worst things I've ever experienced. I offered my plan to Nature, and got a firm smile and a tearing sensation in return-- a piece of myself breaking away, like amethyst dust in the rain.\nI feel lucky to have survived, much less have the sagacity to write this-- I should declare the matter closed, double-check my math before I cast any more Hexes, and never make such a mistake again.".into(), title: None },
                BookPage::Text { text: "...But.\nBut for the scarcest instant, that part of myself... it saw... something. A place-- a design, perhaps? (Such distinctions didn't seem to matter in the face of... that.)\nAnd a... a membrane-barrier-skin-border, separating myself from a realm of raw thought-flow-light-energy. I remember-- I saw-thought-recalled-felt-- the barrier fuzzing at its edges, just so slightly.\nI wanted through.".into(), title: None },
                BookPage::Text { text: "I shouldn't. I know I shouldn't. It's dangerous. It's too dangerous. The force required... I'd have to bring myself within a hair's breadth of Death itself with a single stroke.\nBut I'm. So. Close.\nThis is the culmination of my art. This is the Enlightenment I've been seeking.\nI want more. I need to see it again. I will see it.\nWhat is my mortal mind against immortal glory?".into(), title: None },
            ],
            ..Default::default()
        })
        .add_entry(BookEntry {
            id: "hexcasting:patterns_intro".into(),
            name: "Patterns Introduction".into(),
            category: "hexcasting:patterns".into(),
            pages: vec![
                BookPage::Text { text: "Patterns are the building blocks of every Hex. Each one is a sequence of straight lines drawn across the dots of the casting grid.".into(), title: None },
                BookPage::Text { text: "Draw a pattern by dragging across the grid; release to cast it. Sneak and use the staff to clear your stack.".into(), title: None },
            ],
            ..Default::default()
        })
}
