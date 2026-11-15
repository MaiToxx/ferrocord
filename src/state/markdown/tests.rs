use super::*;
use std::time::{Duration, Instant};

/// Résolveur fictif : noms et couleurs fixes, horodatage `T<secondes><style>`.
struct Fake;

impl Resolve for Fake {
    fn user(&self, id: u64) -> String {
        if id == 42 { "Alice".into() } else { "inconnu".into() }
    }
    fn role(&self, id: u64) -> (String, u32) {
        if id == 5 { ("Modo".into(), 0x3498db) } else { ("rôle".into(), 0) }
    }
    fn channel(&self, id: u64) -> String {
        if id == 100 { "général".into() } else { "salon".into() }
    }
    fn time(&self, secs: i64, style: TimeStyle) -> Option<String> {
        (secs >= 0).then(|| format!("T{secs}{}", style.letter()))
    }
    fn refresh_at(&self, secs: i64) -> Option<i64> {
        Some(secs * 10)
    }
}

fn doc(src: &str) -> Doc {
    parse(src, &Fake)
}

/// Passages d'un paragraphe unique, notés `texte[drapeaux]`.
fn line(src: &str) -> Vec<String> {
    let d = doc(src);
    assert_eq!(d.blocks.len(), 1, "{src:?} : {d:#?}");
    assert_eq!(d.blocks[0].kind, BlockKind::Paragraph, "{src:?}");
    notes(&d, &d.blocks[0])
}

fn notes(d: &Doc, b: &Block) -> Vec<String> {
    b.spans.iter().map(|s| note(d, s)).collect()
}

fn note(d: &Doc, s: &Span) -> String {
    let st = s.style;
    let mut f = String::new();
    for (on, c) in [(st.bold, 'b'), (st.italic, 'i'), (st.underline, 'u'), (st.strike, 's'), (st.code, 'c')] {
        if on {
            f.push(c);
        }
    }
    if let Some(n) = st.spoiler {
        f.push_str(&format!("|{n}"));
    }
    match &s.kind {
        SpanKind::Text => {}
        SpanKind::Link(i) => f.push_str(&format!("→{}", d.links[*i as usize])),
        SpanKind::Mention(m) => f.push_str(&format!("@{m:?}")),
        SpanKind::Time(t) => f.push_str(&format!("t{t}")),
        SpanKind::Emoji { id, animated, .. } => f.push_str(&format!("e{id}{}", if *animated { "a" } else { "" })),
        SpanKind::Unicode(u) => f.push_str(&format!("u:{}", u.name())),
        SpanKind::Code(c) => f.push_str(&format!("{c:?}")),
    }
    if f.is_empty() { s.text.clone() } else { format!("{}[{f}]", s.text) }
}

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// ------------------------------------------------------------------ emphase

/// Emojis Unicode mis à part (une fois, à l'analyse), code en ligne compris,
/// jamais dans un bloc de code.
#[test]
fn unicode_emojis_are_split_out() {
    assert_eq!(line("a 🔥 **b👍🏽**"), v(&["a ", "🔥[u:fire]", " ", "b[b]", "👍🏽[bu:thumbsup_tone3]"]));
    assert_eq!(line("`🔥`  é"), v(&["🔥[cu:fire]", "  é"]));
    assert!(doc("```\n🔥\n```").blocks.iter().flat_map(|b| &b.spans).all(|s| !matches!(s.kind, SpanKind::Unicode(_))));
}

#[test]
fn plain_text_is_one_span() {
    assert_eq!(line("bonjour à tous"), v(&["bonjour à tous"]));
    assert!(doc("").is_empty());
}

#[test]
fn basic_styles() {
    assert_eq!(line("a **gras** b"), v(&["a ", "gras[b]", " b"]));
    assert_eq!(line("*it* et _it_"), v(&["it[i]", " et ", "it[i]"]));
    assert_eq!(line("__sous__"), v(&["sous[u]"]));
    assert_eq!(line("~~barré~~"), v(&["barré[s]"]));
    assert_eq!(line("||caché||"), v(&["caché[|0]"]));
}

#[test]
fn nested_styles() {
    assert_eq!(line("***les deux***"), v(&["les deux[bi]"]));
    assert_eq!(line("___les deux___"), v(&["les deux[iu]"]));
    assert_eq!(line("__*a*__"), v(&["a[iu]"]));
    assert_eq!(line("**gras _et italique_ fin**"), v(&["gras [b]", "et italique[bi]", " fin[b]"]));
    assert_eq!(line("~~**x**~~"), v(&["x[bs]"]));
    assert_eq!(line("||**x** y||"), v(&["x[b|0]", " y[|0]"]));
    assert_eq!(line("**a *b* c**"), v(&["a [b]", "b[bi]", " c[b]"]));
}

#[test]
fn emphasis_boundaries() {
    // Tiret bas au milieu d'un mot : pas d'italique.
    assert_eq!(line("snake_case_name"), v(&["snake_case_name"]));
    assert_eq!(line("a*b*c"), v(&["a", "b[i]", "c"]));
    // Un astérisque seul entouré d'espaces ne fait rien.
    assert_eq!(line("2 * 3 * 4"), v(&["2 * 3 * 4"]));
    // Doubles marqueurs collés au texte qu'ils entourent (comme CommonMark).
    assert_eq!(line("** x **"), v(&["** x **"]));
    // Barres et tildes : les espaces sont permis (spoiler d'une phrase entière).
    assert_eq!(line("|| x ||"), v(&[" x [|0]"]));
    // Un seul tilde ou une seule barre : du texte.
    assert_eq!(line("~a~ |b|"), v(&["~a~ |b|"]));
}

#[test]
fn unclosed_markers_stay_literal() {
    assert_eq!(line("**abc"), v(&["**abc"]));
    assert_eq!(line("||secret"), v(&["||secret"]));
    assert_eq!(line("~~x"), v(&["~~x"]));
    assert_eq!(line("`x"), v(&["`x"]));
    assert_eq!(line("*a **b*"), v(&["*a *", "b[i]"]));
    assert_eq!(line("[lien](https://x"), v(&["[lien](", "https://x[→https://x]"]));
    assert_eq!(line("<@42"), v(&["<@42"]));
}

#[test]
fn escapes() {
    assert_eq!(line(r"\*pas italique\*"), v(&["*pas italique*"]));
    assert_eq!(line(r"\<@42>"), v(&["<@42>"]));
    assert_eq!(line(r"\_a\_ \\ \n"), v(&[r"_a_ \ \n"]));
    assert_eq!(line(r"\# pas un titre"), v(&["# pas un titre"]));
}

// ------------------------------------------------------------------ code

#[test]
fn inline_code() {
    assert_eq!(line("a `**x** <@42>` b"), v(&["a ", "**x** <@42>[c]", " b"]));
    assert_eq!(line("`` a`b ``"), v(&["a`b[c]"]));
    assert_eq!(line("**`x`**"), v(&["x[bc]"]));
}

#[test]
fn code_blocks() {
    let d = doc("avant\n```rust\nfn main() {}\n```\naprès");
    assert_eq!(d.blocks.len(), 3, "{d:#?}");
    assert_eq!(notes(&d, &d.blocks[0]), v(&["avant"]));
    assert_eq!(d.blocks[1].kind, BlockKind::Code { lang: Some("rust".into()), spoiler: None });
    assert_eq!(notes(&d, &d.blocks[1]), v(&["fn[Keyword]", " main() {}[Plain]"]));
    assert_eq!(notes(&d, &d.blocks[2]), v(&["après"]));
    // Sur une ligne, sans langage ; la suite de la ligne est un paragraphe.
    let d = doc("voici ```x = 1``` fin");
    let kinds: Vec<&BlockKind> = d.blocks.iter().map(|b| &b.kind).collect();
    assert_eq!(kinds, vec![&BlockKind::Paragraph, &BlockKind::Code { lang: None, spoiler: None }, &BlockKind::Paragraph]);
    assert_eq!(notes(&d, &d.blocks[1]), v(&["x = 1[Plain]"]));
    assert_eq!(notes(&d, &d.blocks[2]), v(&[" fin"]));
    // Le contenu n'est pas interprété.
    let d = doc("```\n**pas gras** <@42>\n```");
    assert_eq!(notes(&d, &d.blocks[0]), v(&["**pas gras** <@42>[Plain]"]));
    // Non fermé ou vide : du texte.
    assert_eq!(doc("```rust\nfn x").blocks[0].kind, BlockKind::Paragraph);
    assert_eq!(doc("``````").blocks[0].kind, BlockKind::Paragraph);
    // En spoiler.
    let d = doc("||```secret```||");
    assert_eq!(d.blocks.len(), 1, "{d:#?}");
    assert_eq!(d.blocks[0].kind, BlockKind::Code { lang: None, spoiler: Some(0) });
    assert_eq!(d.spoilers, 1);
}

#[test]
fn syntax_classes() {
    let spans = syntax::highlight(Some("py"), "def f(x):  # note\n    return \"s\" + 42");
    let classes: Vec<(String, Class)> = spans.iter().map(|s| (s.text.clone(), if let SpanKind::Code(c) = s.kind { c } else { Class::Plain })).collect();
    assert!(classes.contains(&("def".into(), Class::Keyword)), "{classes:?}");
    assert!(classes.contains(&("# note".into(), Class::Comment)), "{classes:?}");
    assert!(classes.contains(&("\"s\"".into(), Class::String)), "{classes:?}");
    assert!(classes.contains(&("42".into(), Class::Number)), "{classes:?}");
    let spans = syntax::highlight(Some("diff"), "+ajout\n-retrait\n contexte");
    let classes: Vec<Class> = spans.iter().filter_map(|s| if let SpanKind::Code(c) = s.kind { Some(c) } else { None }).collect();
    assert_eq!(classes, vec![Class::Added, Class::Removed, Class::Plain]);
    // Langage inconnu : un seul morceau ; le texte est toujours intact.
    assert_eq!(syntax::highlight(Some("brainfuck"), "+-<>").len(), 1);
    for lang in ["rs", "js", "ts", "json", "sql", "sh", "lua", "css", "cpp", "go", "toml"] {
        let src = "/* é */ let x = \"é\\\"\" + 'c' -- 1.5e3 // fin\n# ünï `t`";
        let out: String = syntax::highlight(Some(lang), src).iter().map(|s| s.text.as_str()).collect();
        assert_eq!(out, src, "{lang}");
    }
}

// ------------------------------------------------------------------ blocs

#[test]
fn quotes() {
    let d = doc("> a\n> **b**\nc");
    assert_eq!(d.blocks.len(), 2, "{d:#?}");
    assert!(d.blocks[0].quote && !d.blocks[1].quote);
    assert_eq!(notes(&d, &d.blocks[0]), v(&["a\n", "b[b]"]));
    // `>>>` : tout le reste du message.
    let d = doc("avant\n>>> un\n# deux\ntrois");
    assert_eq!(d.blocks.len(), 4, "{d:#?}");
    assert!(!d.blocks[0].quote && d.blocks[1..].iter().all(|b| b.quote));
    assert_eq!(d.blocks[2].kind, BlockKind::Heading(1));
    // Bloc de code cité ; pas de citation dans une citation.
    let d = doc("> ```\n> code\n> ```");
    assert_eq!(d.blocks.len(), 1, "{d:#?}");
    assert!(d.blocks[0].quote && matches!(d.blocks[0].kind, BlockKind::Code { .. }));
    let d = doc("> > a");
    assert_eq!(notes(&d, &d.blocks[0]), v(&["> a"]));
    // Sans espace : pas une citation.
    assert!(!doc(">a").blocks[0].quote);
}

#[test]
fn headings_and_subtext() {
    let d = doc("# Un\n## Deux\n### Trois\n#### Quatre\n#Cinq\n-# petit");
    let kinds: Vec<&BlockKind> = d.blocks.iter().map(|b| &b.kind).collect();
    assert_eq!(
        kinds,
        vec![&BlockKind::Heading(1), &BlockKind::Heading(2), &BlockKind::Heading(3), &BlockKind::Paragraph, &BlockKind::Subtext]
    );
    assert_eq!(notes(&d, &d.blocks[3]), v(&["#### Quatre\n#Cinq"]));
    assert_eq!(notes(&d, &d.blocks[0]), v(&["Un"]));
    // Titre vide : du texte.
    assert_eq!(doc("# ").blocks[0].kind, BlockKind::Paragraph);
}

#[test]
fn lists() {
    let d = doc("- a\n- **b**\n  - c\n    - d\n  - e\n- f\n1. x\n1. y\n\n3. z\n4. w\n* étoile\n*italique*");
    let items: Vec<(u8, Marker)> = d
        .blocks
        .iter()
        .filter_map(|b| match b.kind {
            BlockKind::Item { depth, marker } => Some((depth, marker)),
            _ => None,
        })
        .collect();
    use Marker::*;
    assert_eq!(
        items,
        vec![(0, Bullet), (0, Bullet), (1, Bullet), (2, Bullet), (1, Bullet), (0, Bullet), (0, Number(1)), (0, Number(2)), (0, Number(3)), (0, Number(4)), (0, Bullet)]
    );
    assert_eq!(notes(&d, &d.blocks[1]), v(&["b[b]"]));
    let last = d.blocks.last().map(|b| notes(&d, b));
    assert_eq!(last, Some(v(&["italique[i]"])));
    // Imbrication bornée.
    let deep: String = (0..40).map(|i| format!("{}- n{i}\n", " ".repeat(i * 2))).collect();
    let max = doc(&deep).blocks.iter().filter_map(|b| if let BlockKind::Item { depth, .. } = b.kind { Some(depth) } else { None }).max();
    assert_eq!(max, Some(MAX_LIST_DEPTH as u8 - 1));
}

// ------------------------------------------------------------------ liens et mentions

#[test]
fn links() {
    assert_eq!(line("voir https://exemple.invalid/a?b=1."), v(&["voir ", "https://exemple.invalid/a?b=1[→https://exemple.invalid/a?b=1]", "."]));
    assert_eq!(
        line("(https://fr.wikipedia.org/wiki/Rust_(langage))"),
        v(&["(", "https://fr.wikipedia.org/wiki/Rust_(langage)[→https://fr.wikipedia.org/wiki/Rust_(langage)]", ")"])
    );
    assert_eq!(line("[le **site**](https://exemple.invalid)"), v(&["le [→https://exemple.invalid]", "site[b→https://exemple.invalid]"]));
    assert_eq!(line("<https://exemple.invalid/sans-apercu>"), v(&["https://exemple.invalid/sans-apercu[→https://exemple.invalid/sans-apercu]"]));
    assert_eq!(line("[x](<https://exemple.invalid/a b>)"), v(&["[x](<", "https://exemple.invalid/a[→https://exemple.invalid/a]", " b>)"]));
    // Seulement http(s) : le reste est du texte.
    assert_eq!(line("[clic](javascript:alert(1))"), v(&["[clic](javascript:alert(1))"]));
    assert_eq!(line("xhttps://a.b"), v(&["xhttps://a.b"]));
    // Pas de lien dans un lien.
    assert_eq!(line("[https://a.b](https://c.d)"), v(&["https://a.b[→https://c.d]"]));
    assert_eq!(line("||https://a.b||"), v(&["https://a.b[|0→https://a.b]"]));
}

#[test]
fn mentions_and_tags() {
    assert_eq!(line("<@42> <@!42> <@7>"), v(&["@Alice[@User(42)]", " ", "@Alice[@User(42)]", " ", "@inconnu[@User(7)]"]));
    assert_eq!(line("<@&5>"), v(&["@Modo[@Role { id: 5, color: 3447003 }]"]));
    assert_eq!(line("<#100> <#9>"), v(&["#général[@Channel(100)]", " ", "#salon[@Channel(9)]"]));
    assert_eq!(line("@everyone @here"), v(&["@everyone[@Everyone]", " ", "@here[@Here]"]));
    assert_eq!(line("</ping info:77>"), v(&["/ping info[@Command(77)]"]));
    assert_eq!(line("<:rust:123><a:danse:456>"), v(&[":rust:[e123]", ":danse:[e456a]"]));
    assert_eq!(line("<t:1700000000> <t:1700000000:R> <t:1:d>"), v(&["T1700000000f[t1700000000]", " ", "T1700000000R[t1700000000]", " ", "T1d[t1]"]));
    // Balises invalides : laissées telles quelles.
    for raw in ["<t:1:x>", "<t:-5>", "<@abc>", "<:a b:1>", "<#>", "<t:99999999999999999999999>", "<x>"] {
        assert_eq!(line(raw), v(&[raw]), "{raw}");
    }
    assert_eq!(line("**<@42>**"), v(&["@Alice[b@User(42)]"]));
}

#[test]
fn spoilers_are_numbered_in_order() {
    let d = doc("||a|| et ||b **c**||\n> ||d||");
    let ids: Vec<Option<u32>> = d.blocks.iter().flat_map(|b| b.spans.iter().map(|s| s.style.spoiler)).collect();
    assert_eq!(d.spoilers, 3, "{d:#?}");
    assert_eq!(ids, vec![Some(0), None, Some(1), Some(1), Some(2)]);
}

// ------------------------------------------------------------------ entrées hostiles

/// Analyse `src` et vérifie qu'elle reste rapide (aucun temps quadratique :
/// 100 000 caractères en temps quadratique prendraient des secondes).
fn quick(src: &str) -> Doc {
    let start = Instant::now();
    let d = doc(src);
    assert!(start.elapsed() < Duration::from_millis(1500), "{} ms pour {:?}…", start.elapsed().as_millis(), &src[..src.len().min(40)]);
    d
}

#[test]
fn hostile_inputs_stay_linear() {
    let n = 50_000;
    for unit in [
        "*", "_", "**", "__", "~~", "||", "`", "``", "[", "<", "[a](", "[a](http", "<@", "<t:", "*a _b ", "_a *b ", "http://x", "> ", "- ", "```", "\\",
        "||a", "**a", "[](https://a.b ", "<https://a", "@everyone", "\n",
    ] {
        quick(&unit.repeat(n / unit.len().max(1)));
    }
    // Très long texte normal.
    let d = quick(&"mot **gras** et `code` https://exemple.invalid ".repeat(2_000));
    assert_eq!(d.blocks.len(), 1);
}

#[test]
fn deep_nesting_is_bounded() {
    let n = 5_000;
    let src = format!("{}x{}", "*_~~||".repeat(n), "||~~_*".repeat(n));
    let d = quick(&src);
    let text: String = d.blocks.iter().flat_map(|b| b.spans.iter().map(|s| s.text.as_str())).collect();
    assert!(text.contains('x'));
}

/// Aucune entrée ne fait paniquer (caractères spéciaux et multi-octets mêlés).
#[test]
fn random_inputs_never_panic() {
    let alphabet: Vec<&str> = vec![
        "*", "_", "~", "|", "`", "\\", "<", ">", "[", "]", "(", ")", "@", "#", ":", "-", " ", "\n", "> ", "```", "https://", "t:", "1", "a", "é", "😀", "&", "!",
        ".", "everyone", "a:",
    ];
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    for _ in 0..3_000 {
        let len = (seed % 60) as usize;
        let mut s = String::new();
        for _ in 0..len {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            s.push_str(alphabet[(seed % alphabet.len() as u64) as usize]);
        }
        let d = doc(&s);
        for b in &d.blocks {
            for sp in &b.spans {
                if let SpanKind::Link(i) = sp.kind {
                    assert!((i as usize) < d.links.len());
                }
                if let Some(k) = sp.style.spoiler {
                    assert!(k < d.spoilers, "{s:?}");
                }
            }
        }
    }
}

// ------------------------------------------------------------------ horodatages

#[test]
fn timestamp_styles_in_both_languages() {
    use crate::i18n::{time::stamp_in, Lang};
    use chrono::Utc;
    // Mardi 20 avril 2021, 16:20:30 UTC.
    let ms = 1_618_935_630_000;
    let now = ms + 2 * 30 * 86_400_000 + 1000;
    let fr = |s, h24| stamp_in(Lang::Fr, &Utc, ms, now, s, h24);
    let en = |s, h24| stamp_in(Lang::En, &Utc, ms, now, s, h24);
    assert_eq!(fr('t', true), "16:20");
    assert_eq!(fr('T', true), "16:20:30");
    assert_eq!(fr('d', true), "20/04/2021");
    assert_eq!(fr('D', true), "20 avril 2021");
    assert_eq!(fr('f', true), "20 avril 2021 16:20");
    assert_eq!(fr('F', true), "mardi 20 avril 2021 16:20");
    assert_eq!(fr('R', true), "il y a 2\u{a0}mois");
    assert_eq!(en('t', false), "4:20 PM");
    assert_eq!(en('T', false), "4:20:30 PM");
    assert_eq!(en('d', false), "04/20/2021");
    assert_eq!(en('D', false), "April 20, 2021");
    assert_eq!(en('f', false), "April 20, 2021 4:20 PM");
    assert_eq!(en('F', false), "Tuesday, April 20, 2021 4:20 PM");
    assert_eq!(en('R', false), "2 months ago");
    assert_eq!(stamp_in(Lang::En, &Utc, now + 3_600_000, now, 'R', true), "in 1 hour");
    assert_eq!(stamp_in(Lang::Fr, &Utc, now - 5_000, now, 'R', true), "il y a 5\u{a0}secondes");
    // Hors plage : rien (la balise reste alors telle quelle).
    assert_eq!(stamp_in(Lang::Fr, &Utc, i64::MAX, now, 'f', true), "");
}

/// Horodatages relatifs : le document retient le premier instant où l'un
/// d'eux change de texte ; les autres styles n'en demandent pas.
#[test]
fn relative_stamps_ask_for_a_refresh() {
    assert_eq!(doc("<t:5:R> et <t:3:R>, <t:1:f>").refresh_at_ms, Some(30));
    assert_eq!(doc("<t:1:f> <t:2:t>").refresh_at_ms, None);
    use crate::i18n::time::relative_changes_in as next;
    const MIN: i64 = 60_000;
    // Il y a 10 s : « 1 minute » dans 50 s ; il y a 5 min 20 s : « 6 minutes » dans 40 s.
    assert_eq!(next(0, 10_000), 50_000);
    assert_eq!(next(0, 5 * MIN + 20_000), 40_000);
    // Il y a 2 h 30 : à 3 h ; dans 2 h 30 : à 2 h.
    assert_eq!(next(0, 150 * MIN), 30 * MIN);
    assert_eq!(next(150 * MIN, 0), 30 * MIN + 1);
    // Il y a 3 jours et 1 h : le jour suivant.
    assert_eq!(next(0, 73 * 60 * MIN), 23 * 60 * MIN);
}

/// Textes rattachés (réponse, transfert, embeds) : spoilers numérotés à la
/// suite de ceux du message, sans collision.
#[test]
fn attached_texts_continue_spoiler_numbers() {
    let first = doc("||a|| et ||b||");
    assert_eq!(first.spoilers, 2);
    let next = parse_from("||c||", &Fake, first.spoilers);
    assert_eq!(next.blocks[0].spans[0].style.spoiler, Some(2));
    assert_eq!(next.spoilers, 3);
}
