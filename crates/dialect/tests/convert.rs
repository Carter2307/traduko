use dialect::{convert, convert_detailed, ChangeKind, EnglishVariant, Options, Vocabulary};
use EnglishVariant::{American, British};

fn us(text: &str) -> String {
    convert(text, American, &Options::default())
}
fn uk(text: &str) -> String {
    convert(text, British, &Options::default())
}
fn us_spelling(text: &str) -> String {
    convert(text, American, &Options::spelling_only())
}
fn uk_spelling(text: &str) -> String {
    convert(text, British, &Options::spelling_only())
}
fn extended() -> Options {
    Options { vocabulary: Vocabulary::Extended, ..Options::default() }
}

/// Every pair must convert in both directions (spelling layer only).
fn both_ways(pairs: &[(&str, &str)]) {
    for (british, american) in pairs {
        assert_eq!(us_spelling(british), *american, "to American: {british}");
        assert_eq!(uk_spelling(american), *british, "to British: {american}");
    }
}

// ------------------------------------------------------------------ full sentences

#[test]
fn sentence_lorry_flats_lift() {
    let british = "The lorry is parked in front of the block of flats, next to the lift.";
    let american = "The truck is parked in front of the apartment building, next to the elevator.";
    assert_eq!(us(british), american);
    assert_eq!(uk(american), british);
}

#[test]
fn sentence_realised_programme_cancelled() {
    let british = "She realised the television programme had been cancelled.";
    let american = "She realized the television program had been canceled.";
    assert_eq!(us(british), american);
    assert_eq!(uk(american), british);
}

#[test]
fn sentence_favourite_colour_grey_theatre_centre() {
    let british = "My favourite colour is grey and I live near the theatre in the city centre.";
    let american = "My favorite color is gray and I live near the theater in the city center.";
    assert_eq!(us(british), american);
    assert_eq!(uk(american), british);
}

#[test]
fn sentence_with_vocabulary_off_keeps_the_words() {
    let british = "The lorry is parked in front of the block of flats, next to the lift.";
    assert_eq!(us_spelling(british), british);
    let american = "The truck is parked in front of the apartment building, next to the elevator.";
    assert_eq!(uk_spelling(american), american);
}

#[test]
fn shared_test_set_sentences() {
    // English sides of the shared test set (en_to_fr items 5, 6, 8, 9, 12).
    assert_eq!(
        us("The lorry is parked in front of the block of flats, next to the lift."),
        "The truck is parked in front of the apartment building, next to the elevator."
    );
    assert_eq!(
        uk("The researchers found that the model generalizes poorly when the training data is noisy, which limits its use in production."),
        "The researchers found that the model generalises poorly when the training data is noisy, which limits its use in production."
    );
    assert_eq!(
        us("I moved to Lyon three years ago. At first I didn't know anyone, but my neighbours quickly invited me over for dinner."),
        "I moved to Lyon three years ago. At first I didn't know anyone, but my neighbors quickly invited me over for dinner."
    );
}

// ------------------------------------------------------------------ spelling families

#[test]
fn family_our_or() {
    both_ways(&[
        ("colour", "color"),
        ("honour", "honor"),
        ("neighbour", "neighbor"),
        ("behaviour", "behavior"),
        ("flavour", "flavor"),
        ("labour", "labor"),
        ("harbour", "harbor"),
        ("humour", "humor"),
        ("rumour", "rumor"),
        ("favourite", "favorite"),
    ]);
    // no "u" in these derived forms, in either dialect
    for same in ["honorary", "humorous", "vigorous", "glamorous", "laborious", "coloration"] {
        assert_eq!(uk_spelling(same), same);
        assert_eq!(us_spelling(same), same);
    }
}

#[test]
fn family_ise_ize() {
    both_ways(&[
        ("organise", "organize"),
        ("realise", "realize"),
        ("recognise", "recognize"),
        ("apologise", "apologize"),
        ("specialised", "specialized"),
        ("generalises", "generalizes"),
        ("optimisation", "optimization"),
        ("civilisation", "civilization"),
        ("tokenise", "tokenize"),
    ]);
    // words that end in -ise / -ize in BOTH dialects must not move
    for same in [
        "advertise", "advise", "exercise", "surprise", "promise", "rise", "wise", "otherwise", "supervise",
        "compromise", "revise", "franchise", "enterprise", "merchandise", "expertise", "televise", "disguise",
        "improvise", "despise", "precise", "concise", "paradise", "size", "seize", "capsize", "prize",
    ] {
        assert_eq!(us_spelling(same), same, "to American: {same}");
        assert_eq!(uk_spelling(same), same, "to British: {same}");
    }
}

#[test]
fn oxford_spelling_keeps_ize_but_stays_british() {
    let oxford = Options { oxford_ize: true, ..Options::spelling_only() };
    // -ize is kept, and -ise input is normalised to -ize
    assert_eq!(convert("organize", British, &oxford), "organize");
    assert_eq!(convert("organise", British, &oxford), "organize");
    assert_eq!(convert("the organisation realised it", British, &oxford), "the organization realized it");
    // ... but -yse, -our, -re and the rest are British
    assert_eq!(convert("analyze", British, &oxford), "analyse");
    assert_eq!(convert("analyse", British, &oxford), "analyse");
    assert_eq!(
        convert("We organized the color catalog at the center.", British, &oxford),
        "We organized the colour catalogue at the centre."
    );
    assert_eq!(convert("colorize", British, &oxford), "colourize");
    // the option has no effect on American output
    assert_eq!(convert("organise the colour", American, &oxford), "organize the color");
}

#[test]
fn family_yse_yze() {
    both_ways(&[("analyse", "analyze"), ("analysed", "analyzed"), ("analysing", "analyzing"), ("paralysed", "paralyzed"), ("catalyse", "catalyze")]);
}

#[test]
fn family_re_er() {
    both_ways(&[
        ("centre", "center"),
        ("centres", "centers"),
        ("centred", "centered"),
        ("theatre", "theater"),
        ("fibre", "fiber"),
        ("litre", "liter"),
        ("kilometre", "kilometer"),
        ("calibre", "caliber"),
        ("sombre", "somber"),
        ("spectre", "specter"),
        ("lustre", "luster"),
        ("meagre", "meager"),
    ]);
    for same in ["acre", "massacre", "mediocre", "genre", "ogre", "letter", "number", "parameter", "thermometer", "diameter"] {
        assert_eq!(us_spelling(same), same);
        assert_eq!(uk_spelling(same), same);
    }
}

#[test]
fn family_double_l() {
    both_ways(&[
        ("travelled", "traveled"),
        ("travelling", "traveling"),
        ("traveller", "traveler"),
        ("cancelled", "canceled"),
        ("modelling", "modeling"),
        ("jewellery", "jewelry"),
        ("counsellor", "counselor"),
        ("marvellous", "marvelous"),
        ("fuelled", "fueled"),
        ("labelled", "labeled"),
        ("woollen", "woolen"),
        // the other way round: one l in British
        ("enrol", "enroll"),
        ("fulfil", "fulfill"),
        ("skilful", "skillful"),
        ("instalment", "installment"),
        ("wilful", "willful"),
    ]);
    for same in ["cancellation", "controlled", "compelled", "rebelled", "installed", "spelling", "killing"] {
        assert_eq!(us_spelling(same), same);
        assert_eq!(uk_spelling(same), same);
    }
}

#[test]
fn family_ogue_ence_ae_oe() {
    both_ways(&[
        ("catalogue", "catalog"),
        ("defence", "defense"),
        ("offence", "offense"),
        ("pretence", "pretense"),
        ("anaemia", "anemia"),
        ("paediatric", "pediatric"),
        ("oestrogen", "estrogen"),
        ("manoeuvre", "maneuver"),
        ("haemorrhage", "hemorrhage"),
        ("diarrhoea", "diarrhea"),
    ]);
    // "dialogue" and "analogue" are also American spellings: left alone going to American
    assert_eq!(us_spelling("dialogue"), "dialogue");
    assert_eq!(us_spelling("analogue"), "analogue");
    assert_eq!(uk_spelling("analog"), "analogue");
    // defensive / offensive keep the s everywhere
    assert_eq!(uk_spelling("defensive offensive"), "defensive offensive");
}

#[test]
fn single_words() {
    both_ways(&[
        ("grey", "gray"),
        ("aluminium", "aluminum"),
        ("aeroplane", "airplane"),
        ("plough", "plow"),
        ("cosy", "cozy"),
        ("sceptical", "skeptical"),
        ("moustache", "mustache"),
        ("pyjamas", "pajamas"),
        ("mould", "mold"),
        ("maths", "math"),
        ("sulphur", "sulfur"),
        ("artefact", "artifact"),
        ("speciality", "specialty"),
        ("ageing", "aging"),
        ("judgement", "judgment"),
        ("acknowledgement", "acknowledgment"),
        ("chequebook", "checkbook"),
        ("draughty", "drafty"),
    ]);
}

#[test]
fn inflections_and_derived_forms() {
    both_ways(&[
        ("colours", "colors"),
        ("coloured", "colored"),
        ("colouring", "coloring"),
        ("colourful", "colorful"),
        ("colourless", "colorless"),
        ("discoloured", "discolored"),
        ("multicoloured", "multicolored"),
        ("organised", "organized"),
        ("organising", "organizing"),
        ("organiser", "organizer"),
        ("organisation", "organization"),
        ("organisations", "organizations"),
        ("organisational", "organizational"),
        ("reorganised", "reorganized"),
        ("disorganised", "disorganized"),
        ("unorganised", "unorganized"),
        ("neighbourhood", "neighborhood"),
        ("neighbouring", "neighboring"),
        ("honourable", "honorable"),
        ("favourably", "favorably"),
        ("behavioural", "behavioral"),
        ("centrepiece", "centerpiece"),
        ("theatregoer", "theatergoer"),
    ]);
}

#[test]
fn prefix_fallback() {
    // not in the tables as such: prefix + a word of the tables
    both_ways(&[
        ("uncoloured", "uncolored"),
        ("recolouring", "recoloring"),
        ("overemphasised", "overemphasized"),
        ("nonlabour", "nonlabor"),
        ("precancelled", "precanceled"),
        ("misbehaviour", "misbehavior"),
        ("semicoloured", "semicolored"),
    ]);
    // a prefix alone proves nothing
    for same in ["remeter", "demeter", "prefer", "recenter", "uncle", "render"] {
        let _ = (us_spelling(same), uk_spelling(same)); // must not panic
    }
    assert_eq!(uk_spelling("prefer"), "prefer");
    assert_eq!(uk_spelling("uncle render"), "uncle render");
}

// ------------------------------------------------------------------ case, possessives, hyphens

#[test]
fn case_is_preserved() {
    assert_eq!(us("colour"), "color");
    assert_eq!(us("Colour is nice."), "Color is nice.");
    assert_eq!(us("COLOUR"), "COLOR");
    assert_eq!(uk("Color is nice."), "Colour is nice.");
    assert_eq!(uk("COLOR"), "COLOUR");
    assert_eq!(uk("WE ORGANIZED THE CENTER."), "WE ORGANISED THE CENTRE.");
    // sentence starts after . ! ? and line breaks, also inside quotes and after a bullet
    assert_eq!(us("It rains. Grey skies again! Favourite weather? Honestly no."), "It rains. Gray skies again! Favorite weather? Honestly no.");
    assert_eq!(us("First line\nColour everywhere\n- Grey walls\n1. Centre stage"), "First line\nColor everywhere\n- Gray walls\n1. Center stage");
    assert_eq!(us("\"Colour is life,\" she said."), "\"Color is life,\" she said.");
    // vocabulary swaps follow the case too
    assert_eq!(us("Lorry drivers are on strike."), "Truck drivers are on strike.");
    assert_eq!(uk("TRUCK"), "LORRY");
    assert_eq!(uk("Apartment buildings are tall."), "Blocks of flats are tall.");
}

#[test]
fn possessives() {
    assert_eq!(us("the colour's intensity"), "the color's intensity");
    assert_eq!(us("the neighbour\u{2019}s dog"), "the neighbor\u{2019}s dog");
    assert_eq!(us("my neighbours' gardens"), "my neighbors' gardens");
    assert_eq!(uk("the organization's center"), "the organisation's centre");
    assert_eq!(us("the lorry's driver"), "the truck's driver");
    assert_eq!(uk("the traveler's check"), "the traveller's cheque");
    assert_eq!(uk("my driver's license"), "my driving licence");
    assert_eq!(us("my driving licence"), "my driver's license");
}

#[test]
fn hyphenated_words() {
    assert_eq!(us("a colour-coded, well-organised, grey-haired plan"), "a color-coded, well-organized, gray-haired plan");
    assert_eq!(uk("a color-coded, well-organized, gray-haired plan"), "a colour-coded, well-organised, grey-haired plan");
    assert_eq!(uk("a two-story house and a 12-story tower"), "a two-storey house and a 12-storey tower");
    assert_eq!(us("a multi-storey car park"), "a parking garage");
    assert_eq!(us_spelling("a multi-storey car park"), "a multi-story car park");
}

#[test]
fn apostrophes_and_quotes_are_word_boundaries() {
    assert_eq!(us("I don't like it, it's 'colour' and \u{2018}grey\u{2019}."), "I don't like it, it's 'color' and \u{2018}gray\u{2019}.");
    assert_eq!(us("\u{201C}colour\u{201D} (colour) [colour] colour, colour; colour: colour! colour?"), "\u{201C}color\u{201D} (color) [color] color, color; color: color! color?");
    assert_eq!(us("colour\u{2014}grey\u{2013}centre/theatre"), "color\u{2014}gray\u{2013}center/theater");
    assert_eq!(us("o'clock rock'n'roll l'organisation"), "o'clock rock'n'roll l'organisation");
}

// ------------------------------------------------------------------ never touched

#[test]
fn urls_emails_and_code_are_left_alone() {
    let text = "See https://example.co.uk/colour/centre?favourite=grey and www.theatre.org or mail colour@centre.org.";
    assert_eq!(us(text), text);
    let text = "Set favourite_colour, backgroundColour, COLOUR_MODE, --colour, colour.rs, src/colour/mod.rs, colour(), #colour and $colour.";
    assert_eq!(us(text), text);
    let text = "Run `cargo run --colour always` then ```\nlet colour = grey;\n``` done.";
    assert_eq!(us(text), text);
    // prose around code is still converted
    assert_eq!(us("The colour is set in `colour.rs`; see https://x.org/colour for the colour list."), "The color is set in `colour.rs`; see https://x.org/colour for the color list.");
    // mixed-case and letter+digit tokens are identifiers, not words
    assert_eq!(us("iColour McColour colour2 h264"), "iColour McColour colour2 h264");
}

#[test]
fn proper_nouns_keep_their_spelling() {
    // a Capitalised word that does not start the sentence is a name
    let text = "The Labour Party won, said the World Health Organization at Pearl Harbor.";
    assert_eq!(us(text), text);
    assert_eq!(uk(text), text);
    let text = "We visited the Centre Pompidou, the Globe Theatre, Lincoln Center and the Sydney Harbour Bridge.";
    assert_eq!(us(text), text);
    assert_eq!(uk(text), text);
    // the first word of a sentence followed by another Capitalised word is a name too
    assert_eq!(us("Labour Party members voted."), "Labour Party members voted.");
    assert_eq!(uk("Pearl Harbor was attacked."), "Pearl Harbor was attacked.");
    assert_eq!(uk("Labor Day is in September."), "Labor Day is in September.");
    // "Labour" + a political word, even at the start of a sentence
    assert_eq!(us("Labour MPs voted against the programme."), "Labour MPs voted against the program.");
    assert_eq!(us("Labour leaders organised a meeting."), "Labour leaders organized a meeting.");
    // lower-case uses are ordinary words
    assert_eq!(us("the labour market and the harbour"), "the labor market and the harbor");
    // "I" does not count as a name
    assert_eq!(us("Realising I was late, I ran."), "Realizing I was late, I ran.");
    // the guard can be switched off
    let off = Options { protect_proper_nouns: false, ..Options::default() };
    assert_eq!(convert("My Favourite Colour", American, &off), "My Favorite Color");
    assert_eq!(us("My Favourite Colour"), "My Favourite Colour");
}

// ------------------------------------------------------------------ homographs (spelling layer)

#[test]
fn check_is_not_cheque_without_context() {
    assert_eq!(uk("Please check the door and do a quick check."), "Please check the door and do a quick check.");
    assert_eq!(uk("Can I pay by check?"), "Can I pay by cheque?");
    assert_eq!(uk("She wrote a check for $500."), "She wrote a cheque for $500.");
    assert_eq!(uk("a check for errors"), "a check for errors");
    assert_eq!(us("a cheque for 50 pounds"), "a check for 50 pounds");
}

#[test]
fn tire_tyre() {
    assert_eq!(uk("He never tires of it and they tire easily."), "He never tires of it and they tire easily.");
    assert_eq!(uk("I have a flat tire and the tires are worn."), "I have a flat tyre and the tyres are worn.");
    assert_eq!(uk("Check the tire pressure."), "Check the tyre pressure.");
    assert_eq!(us("a flat tyre"), "a flat tire");
    assert_eq!(uk("tired tireless tiresome"), "tired tireless tiresome");
}

#[test]
fn program_programme() {
    assert_eq!(uk("a training program"), "a training programme");
    assert_eq!(uk("the programs on TV tonight"), "the programmes on TV tonight");
    assert_eq!(uk("This computer program is slow."), "This computer program is slow.");
    assert_eq!(uk("I wrote a program in Python."), "I wrote a program in Python.");
    assert_eq!(uk("You can program the oven."), "You can program the oven.");
    assert_eq!(uk("programmed programming programmer"), "programmed programming programmer");
    assert_eq!(us("the programme and the programmes"), "the program and the programs");
}

#[test]
fn other_homographs() {
    // story / storey
    assert_eq!(uk("a good story and a three-story building, ten stories high"), "a good story and a three-storey building, ten storeys high");
    assert_eq!(us("a three-storey building with two storeys"), "a three-story building with two stories");
    // curb / kerb
    assert_eq!(uk("We must curb inflation. A curb on spending. He sat on the curb."), "We must curb inflation. A curb on spending. He sat on the kerb.");
    // meter / metre
    assert_eq!(uk("The parking meter is 5 meters away, about a meter from the wall, two meters wide."), "The parking meter is 5 metres away, about a metre from the wall, two metres wide.");
    assert_eq!(us("ten metres and a kilometre"), "ten meters and a kilometer");
    // practice / practise
    assert_eq!(uk("I need to practice. Practice makes perfect. She practiced a lot. In practice it works."), "I need to practise. Practice makes perfect. She practised a lot. In practice it works.");
    assert_eq!(us("I practise daily and she practised."), "I practice daily and she practiced.");
    // license / licence
    assert_eq!(uk("a software license; they license the software; a licensed driver"), "a software licence; they license the software; a licensed driver");
    assert_eq!(us("a driving licence"), "a driver's license");
    assert_eq!(us_spelling("a driving licence"), "a driving license");
    // draft / draught
    assert_eq!(uk("a first draft and a draft beer"), "a first draft and a draught beer");
    assert_eq!(us("a cold draught"), "a cold draft");
    assert_eq!(us("They played draughts."), "They played draughts.");
    // analyses: plural noun or verb
    assert_eq!(us("These analyses are wrong. The report analyses the causes."), "These analyses are wrong. The report analyzes the causes.");
    // prize / prise, mum
    assert_eq!(uk("my prized possession won a prize"), "my prized possession won a prize");
}

// ------------------------------------------------------------------ vocabulary layer

#[test]
fn flat_only_as_a_noun() {
    assert_eq!(us("I rent a flat in London."), "I rent an apartment in London.");
    assert_eq!(us("My flat is small. Their flat has two rooms."), "My apartment is small. Their apartment has two rooms.");
    assert_eq!(us("The flats near the station are expensive."), "The apartments near the station are expensive.");
    for same in [
        "The road is flat.",
        "a flat surface",
        "a flat tyre",
        "a flat rate of 10%",
        "her flat shoes",
        "my flat screen",
        "the flat of his hand",
        "The beer went flat.",
        "She fell flat on her face.",
        "the mud flats at low tide",
        "B flat major",
    ] {
        assert_eq!(us_spelling(same).replace("tire", "tyre"), same, "spelling only: {same}");
        assert_eq!(us(same).replace("tire", "tyre"), same, "safe vocabulary: {same}");
    }
}

#[test]
fn lift_only_as_the_machine() {
    assert_eq!(us("Take the lift to the third floor."), "Take the elevator to the third floor.");
    assert_eq!(us("The lifts are out of order."), "The elevators are out of order.");
    for same in [
        "Can you lift this box?",
        "He lifts weights.",
        "She gave me a lift to the station.",
        "Thanks for the lift!",
        "I need a lift home.",
        "The lift of the ban was welcome.",
        "They lifted the ban.",
        "We took the ski lift.",
    ] {
        assert_eq!(us(same), same, "{same}");
    }
}

#[test]
fn unsafe_words_are_left_alone() {
    // Safe tier, both directions: none of these moves.
    for same in [
        "Please check the bill.",
        "He wears pants.",
        "fish and chips",
        "chocolate chips",
        "We played football.",
        "natural gas and tear gas",
        "Leaves fall in the fall of an empire.",
        "a dollar bill and a phone bill",
        "This site uses cookies.",
        "a biscuit",
        "the trunk, the boot and the bonnet",
        "I was mad about it.",
        "a rubber band",
        "the first floor",
        "the subway",
        "a torch",
        "He is quite pissed.",
        "public school",
    ] {
        assert_eq!(us(same), same, "to American: {same}");
        assert_eq!(uk(same), same, "to British: {same}");
    }
    assert_eq!(us("Monday is a bank holiday."), "Monday is a bank holiday.");
    assert_eq!(us("We are on holiday in Spain."), "We are on vacation in Spain.");
    assert_eq!(uk("We are on vacation in Spain."), "We are on holiday in Spain.");
}

#[test]
fn safe_vocabulary_pairs() {
    let pairs = [
        ("a lorry", "a truck"),
        ("two lorries", "two trucks"),
        ("the petrol station", "the gas station"),
        ("on the pavement", "on the sidewalk"),
        ("a mobile phone", "a cell phone"),
        ("the car park", "the parking lot"),
        ("nappies", "diapers"),
        ("an aubergine", "an eggplant"),
        ("the windscreen", "the windshield"),
        ("the number plate", "the license plate"),
        ("candy floss", "cotton candy"),
        ("at the weekend", "on the weekend"),
    ];
    for (british, american) in pairs {
        assert_eq!(us(british), american, "to American: {british}");
        assert_eq!(uk(american), british, "to British: {american}");
    }
    // one-way swaps
    assert_eq!(us("my flatmate"), "my roommate");
    assert_eq!(uk("my roommate"), "my roommate"); // may share the room, not the flat: extended tier
    assert_eq!(uk("a takeout"), "a takeaway");
    assert_eq!(us("the key takeaway"), "the key takeaway");
    assert_eq!(us("Mum's the word."), "Mum's the word.");
    assert_eq!(us("My mum's car"), "My mom's car");
    assert_eq!(us("The lift in sales was welcome."), "The lift in sales was welcome.");
    assert_eq!(us("I ran out of petrol."), "I ran out of gas.");
    assert_eq!(us("the smell of petrol"), "the smell of gasoline");
    assert_eq!(uk("the smell of gasoline"), "the smell of petrol");
    assert_eq!(us("whilst waiting a fortnight"), "while waiting two weeks");
    assert_eq!(uk("in the fall"), "in the autumn");
    assert_eq!(uk("in the fall of 2019"), "in the fall of 2019");
    // guards on truck / elevator
    assert_eq!(uk("a pickup truck, a fire truck and a food truck"), "a pickup truck, a fire truck and a food truck");
    assert_eq!(uk("an elevator pitch"), "an elevator pitch");
    assert_eq!(uk("an elevator"), "a lift");
    assert_eq!(us("a lift shaft is not handled"), "a lift shaft is not handled");
}

#[test]
fn extended_vocabulary_is_opt_in() {
    let x = extended();
    assert_eq!(us("We played football and ate biscuits."), "We played football and ate biscuits.");
    assert_eq!(convert("We played football and ate biscuits.", American, &x), "We played soccer and ate cookies.");
    assert_eq!(convert("American football is popular.", American, &x), "American football is popular.");
    assert_eq!(convert("cheese and biscuits", American, &x), "cheese and biscuits");
    assert_eq!(convert("I love soccer.", British, &x), "I love football.");
    assert_eq!(convert("Soccer is not football here.", British, &x), "Soccer is not football here.");
    assert_eq!(convert("They played draughts.", American, &x), "They played checkers.");
    // cookies are never turned into biscuits (web cookies)
    assert_eq!(convert("This site uses cookies.", British, &x), "This site uses cookies.");
}

// ------------------------------------------------------------------ general properties

#[test]
fn text_already_in_the_target_variant_is_unchanged() {
    let american = "The truck is parked near the theater; my favorite color is gray, and the organization canceled the program.";
    assert_eq!(us(american), american);
    let british = "The lorry is parked near the theatre; my favourite colour is grey, and the organisation cancelled the programme.";
    assert_eq!(uk(british), british);
}

#[test]
fn conversion_is_idempotent() {
    let samples = [
        "The lorry is parked in front of the block of flats, next to the lift.",
        "I rent a flat in London and my neighbours organised a colourful programme.",
        "The truck is parked in front of the apartment building, next to the elevator.",
        "She wrote a check for $500 and bought two tires, 5 meters of cable and a license.",
    ];
    for s in samples {
        for to in [American, British] {
            for opts in [Options::default(), Options::spelling_only(), extended()] {
                let once = convert(s, to, &opts);
                assert_eq!(convert(&once, to, &opts), once, "{to:?} {opts:?}: {s}");
            }
        }
    }
}

#[test]
fn whitespace_unicode_and_empty_input() {
    assert_eq!(us(""), "");
    assert_eq!(us("   "), "   ");
    assert_eq!(us("colour\t\tcentre  \n\n  grey\u{00A0}theatre"), "color\t\tcenter  \n\n  gray\u{00A0}theater");
    assert_eq!(us("Le caf\u{00E9} est gris \u{2014} na\u{00EF}ve colour \u{1F3A8} centre \u{4E2D}\u{5FC3}"), "Le caf\u{00E9} est gris \u{2014} na\u{00EF}ve color \u{1F3A8} center \u{4E2D}\u{5FC3}");
}

#[test]
fn changes_are_reported_with_offsets() {
    let out = convert_detailed("I rent a flat near the theatre.", American, &Options::default());
    assert_eq!(out.text, "I rent an apartment near the theater.");
    let kinds: Vec<ChangeKind> = out.changes.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, vec![ChangeKind::Article, ChangeKind::Vocabulary, ChangeKind::Spelling]);
    for c in &out.changes {
        assert_eq!(&out.text[c.start..c.end], c.to);
    }
    assert_eq!(out.changes[1].from, "flat");
    assert_eq!(out.changes[1].to, "apartment");
    let none = convert_detailed("Nothing to do here.", British, &Options::default());
    assert!(none.changes.is_empty());
}

// ------------------------------------------------------------------ known wrong conversions
// These tests pin the CURRENT behaviour of cases that are not solved. If one of them
// starts failing because the converter got better, update the expectation.

#[test]
fn known_limitations() {
    // 1. A proper noun that is the first word of a sentence and is followed by a lower-case word.
    assert_eq!(us("Labour won the election."), "Labor won the election."); // wrong: the party is "Labour"
    assert_eq!(us("Grey was appointed minister."), "Gray was appointed minister."); // wrong if Grey is a surname
    // 2. A title in Title Case is taken for a list of names and is not converted.
    assert_eq!(us("My Favourite Colours"), "My Favourite Colours");
    // 3. "program" (software) with no computing word in the sentence.
    assert_eq!(uk("The program stopped responding."), "The programme stopped responding.");
    // 4. Homographs with no cue stay in the source spelling.
    assert_eq!(uk("I bought new tires."), "I bought new tyres."); // solved by the "new" cue ...
    assert_eq!(uk("Tires are expensive."), "Tires are expensive."); // ... not solved without a cue
    assert_eq!(uk("He paid with a check."), "He paid with a check."); // should be "cheque"
    assert_eq!(uk("The story has three stories."), "The story has three stories."); // 2nd should be "storeys"
    // 5. "flat" as a noun without one of the listed determiners.
    assert_eq!(us("Flats in Paris are small."), "Flats in Paris are small.");
    // 6. "a flat and ..." keeps the adjective reading, so the noun is missed here.
    assert_eq!(us("She has a flat and a car."), "She has a flat and a car.");
    // 7. Grammar differences are out of scope.
    assert_eq!(us("I have just eaten. He is in hospital."), "I have just eaten. He is in hospital.");
    assert_eq!(uk("I already ate. He has gotten better."), "I already ate. He has gotten better.");
}

#[test]
fn a_name_seen_once_protects_the_same_word_at_a_sentence_start() {
    assert_eq!(
        us("Mr. Grey lives near Harbour Street. Grey is his favourite colour."),
        "Mr. Grey lives near Harbour Street. Grey is his favorite color."
    );
    assert_eq!(
        us("The Labour Party lost. Labour won the last election, when labour costs were low."),
        "The Labour Party lost. Labour won the last election, when labor costs were low."
    );
}

#[test]
fn a_lonely_backtick_does_not_switch_the_converter_off() {
    // man-page style quotes and an unclosed backtick are plain punctuation
    assert_eq!(us("the `colour' option and the grey centre"), "the `color' option and the gray center");
    assert_eq!(us("a stray ` backtick, then colour\nand grey on the next line"), "a stray ` backtick, then color\nand gray on the next line");
    // closed spans and fenced blocks stay protected, the prose after them is converted
    assert_eq!(us("use `set colour` for colour"), "use `set colour` for color");
    assert_eq!(us("```\ncolour = grey\n```\nThe colour is grey."), "```\ncolour = grey\n```\nThe color is gray.");
}

#[test]
fn real_machine_translation_output_is_mixed_and_gets_normalised() {
    // English produced by a Marian fr->en model for the shared test set (items 5, 6, 8, 12):
    // one sentence mixes "favorite color ... theater" with "grey ... centre".
    let mt = "My favorite color is grey and I live in the city centre, next to the theater.";
    assert_eq!(us(mt), "My favorite color is gray and I live in the city center, next to the theater.");
    assert_eq!(uk(mt), "My favourite colour is grey and I live in the city centre, next to the theatre.");
    let mt = "She realized that the TV show had been cancelled.";
    assert_eq!(us(mt), "She realized that the TV show had been canceled.");
    assert_eq!(uk(mt), "She realised that the TV show had been cancelled.");
    let mt = "The truck is parked in front of the building, near the elevator.";
    assert_eq!(us(mt), mt);
    assert_eq!(uk(mt), "The lorry is parked in front of the building, near the lift.");
    assert_eq!(uk_spelling(mt), mt);
}
