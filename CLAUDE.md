# CLAUDE.md — upute za rad na OpenGammonu

Ovaj dokument čita Claude Code na početku svake sesije.
**Izvor istine za opseg i faze je `OPENGAMMON.md`.** Ovdje su samo operativna pravila: kako se
radi, što se smije, što se ne smije i kako izgleda gotov posao.

---

## 0. Stanje projekta

> Ovaj odjeljak se ažurira na kraju svake sesije. Ako je zastario, prvo pitaj.

- **Trenutna faza:** 1 — Move generator
- **Zadnje zatvoreno:** Faza 0 — Temelji (workspace, CI zelen na GitHubu, licenca, CONTRIBUTING, kostur svih crateova)
- **Otvoreno / u tijeku:**
  - `Position` (`[i8; 24]`, relativno prema igraču na potezu, konvencija dokumentirana na tipu) + `generate_moves`/`apply` implementirani u `og-core`; svih 7 imenovanih rubnih testova iz OPENGAMMON.md §4 (i CLAUDE.md §5) prolazi. Dodan `Position::mirror()` (zamjena perspektive, involucija) — bio nedostajući dio za self-play.
  - Generator nasumičnih ali *legalnih* (doigrivih) pozicija gotov: `crates/og-core/src/self_play.rs` igra nasumičnu partiju od startne pozicije. `diff_test_sample()` je jedini izvor istine za seed i raspodjelu poteza (lijeni iterator, ne `Vec` — bitno za milijunski uzorak), i `gnubg_diff.rs` i analize u `self_play.rs` čitaju iz njega. Raspodjela po fazama na uzorku od 10 000 (seed `0xb0ad1ce`, broj poteza ~ U(0,120)): kontakt 84.5%, čista trka 15.5%, bearoff-eligible 19.0% (neovisna brojka), kamen na baru 39.3% (neovisna brojka).
  - GNUbg diferencijalni harness prebačen s "novi proces po svakom pozivu" na dugoživuću sesiju: `GnubgSession` (gnubg_diff.rs) drži jedan `gnubg-cli` proces kroz petlju zahtjev/odgovor (`gnubg_harness.py`). Usput otkriven i popravljen deadlock: GNUbg-ov C-level read za "new session" prompt i Pythonov `sys.stdin` natjecali su se za isti stdin; riješeno READY-handshakeom prije prvog pravog zahtjeva (nalaz u `docs/rules-notes.md`). `hint()` sad radi na 0-ply evaluaciji (samo enumeracija poteza, ne rangiranje) — izmjereno ~2.5× brže, potvrđeno da ne mijenja skup poteza.
  - Provjera da `MAX_MOVES=5000` ne skraćuje popis na gušćim pozicijama: potvrđeno na 638 parova (pozicija, bacanje) — 8 imenovanih rubnih testova + 30×21 iz self-play uzorka, nasuprot capa 200000. Nalaz u `docs/rules-notes.md`.
  - **Milijunski test (`random_self_play_positions_match_gnubg`, `OG_DIFF_SAMPLE_SIZE` override) ostaje neodrađen — 10k probni run još nije prošao do kraja ni jednom.** Dva pokušaja pala na ~2000–3000 pozicija s "gnubg-cli exited before answering a request" (prazan stderr) — uzrok nije potvrđen. Dvije hipoteze na stolu:
    1. konkurentna `cargo build`/`cargo test` naredba usred runa zamijenila je testnu binarku (exit 127) — sad pokriveno pravilom u §3 (nijedna druga `cargo` naredba dok dugi test vrti);
    2. rast memorije 32-bitnog `gnubg-cli.exe` procesa preko desetaka tisuća `hint()` poziva (recikliranje sesije bilo bi rješenje, ali se **ne gradi dok uzrok nije potvrđen**).
    Treći, izolirani pokušaj (bez ijedne konkurentne `cargo` naredbe, uz praćenje memorije procesa) prekinut je na korisnikov zahtjev prije završetka, radi drugih prioriteta u sesiji. **Sljedeći korak: ponoviti taj izolirani pokušaj do kraja** (ili do pada) prije bilo kakvog zaključka o uzroku ili daljnjeg skaliranja.
  - Neistraženo, uočeno usput: `cargo test -p og-core --lib` (paralelno, debug profil) jednom je pao s exit `0xffffffff` dok je `self_play::tests::phase_distribution_of_the_diff_test_sample` radio usporedno s ostalim testovima. Sumnja da je uzrok duboka rekurzija u `collect_plies` je **opovrgnuta**: dubina je dokazano i izmjereno ograničena na točno 5 (dubleti), neovisno o faktoru grananja — vidi `docs/rules-notes.md`. Vjerojatniji uzrok: taj test je analiza (10 000 self-play partija po do 120 poteza), ne provjera, i pretežak je za jednu nit u debug-modu usporedno s ostalim testovima — sad označen `#[ignore]`. Nalaz i otvorena hipoteza u `docs/backlog.md`.
  - Ostaje do definicije "gotovo" Faze 1 (milijun pozicija × 21 bacanje, identičan skup):
    - čist, dovršen 10k probni run (bez konkurentnih `cargo` naredbi) prije skaliranja dalje
    - paralelizacija diferencijalnog runa — pri ~17ms/upit (0-ply, izmjereno) jedna sekvencijalna sesija na milijun pozicija × 21 bacanje treba red veličine 100 sati; ne skalira bez paralelnih `GnubgSession`-a
    - sam milijunski run
- **Poznati dug:** WASM build provjera za `og-core` u CI-ju još nije dodana (vidi `docs/backlog.md`) — uvjet za dodavanje ("kad `og-core` dobije stvarni kod") sad je zadovoljen, pa je ovo sljedeće za pokupiti. Uz to, gornji neistraženi pad paralelnih testova.

---

## 1. Prvo pravilo: faza po faza

Radi se isključivo na trenutnoj fazi iz odjeljka 0. Svaka faza ima definiciju "gotovo" u
`OPENGAMMON.md` §4.

- Ne počinji sljedeću fazu dok definicija "gotovo" nije **mjerljivo** zadovoljena.
- Ako se usput pojavi ideja za kasniju fazu, zapiši je u `docs/backlog.md` i nastavi.
- Ako zadatak koji dobiješ pripada kasnijoj fazi, reci to prije nego počneš pisati kod.

"Skoro gotovo" nije gotovo. Definicija je binarna namjerno.

---

## 2. Tvrda pravila (nema iznimke)

1. **Nikad ne kopiraj GNUbg kod, ni djelomično, ni parafrazirano.** GNUbg je GPLv3; licenca bi
   se prenijela na cijeli projekt. GNUbg se poziva **isključivo kao vanjski proces**, i to samo
   iz testova i `og-eval`. Ni jedan crate nema GNUbg kao ovisnost.
   - Čitanje GNUbg izvornog koda radi razumijevanja formata ili rubnog slučaja je u redu.
     Prepisivanje strukture, tablica ili funkcija nije.
2. **Nikad ne treniraj na GNUbg ili XG evaluacijama.** Labele od njih znače njihov plafon.
   Njihovi rolloutovi idu isključivo u validaciju.
3. **Tolerancija u move generatoru je nula.** Ne "99.99%". Identičan skup poteza.
4. **Bearoff baze računamo sami.** Ne preuzimaj tuđe datoteke, ni za testiranje.
5. **U Fazi 7: LLM nikad ne procjenjuje poziciju.** Dobiva poziciju, kandidate i equity iz
   motora i samo prevodi u tekst. Ako nema izlaz motora, nema ni objašnjenja.

Ako neki zadatak traži kršenje ovih pravila, stani i reci to umjesto da improviziraš.

---

## 3. Naredbe

```bash
cargo test --workspace            # sve
cargo test -p og-core             # jedan crate
cargo test -- --ignored           # spori testovi (milijun pozicija, GNUbg usporedba)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo bench -p og-engine
```

- CI vrti `fmt`, `clippy -D warnings` i `cargo test --workspace` na svaki push.
- Spori testovi (`#[ignore]`) vrte se u zasebnom CI jobu, ne na svaki push.
- Prije nego kažeš da je nešto gotovo, pokreni testove. Ne pretpostavljaj da prolaze.
- **Dok dugi test (npr. GNUbg diferencijalni run) vrti u pozadini, ne pokreće se nijedna
  druga `cargo` naredba** — konkurentni `cargo build`/`cargo test` zna zamijeniti testnu
  binarku usred runa, pa taj proces padne s exit 127. Pričekaj da pozadinski run završi
  prije sljedećeg `cargo` poziva.

---

## 4. Konvencije koda

**Jezik.** Kod, imena, komentari, commit poruke i dokumentacija u repozitoriju su na
engleskom — projekt je otvoren i ciljna zajednica je međunarodna. Razgovor sa mnom je na
hrvatskom. (Ako ti se ovo ne sviđa, promijeni pravilo ovdje i drži se toga dosljedno.)

**Granice crateova.** Ovisnosti idu samo prema dolje:

```
og-core  ←  og-bearoff  ←  og-engine  ←  og-rollout
   ↑            ↑              ↑             ↑
   └──────── og-formats, og-eval, og-wasm, og-cli
```

`og-core` ne ovisi ni o čemu iz workspacea. Ako ti treba obrnuta ovisnost, dizajn je kriv.

**Vanjske ovisnosti.** Svaka nova ovisnost se prvo predloži pa doda. `og-core` i `og-bearoff`
ciljaju nula ili gotovo nula ovisnosti — moraju čisto u WASM.

**Greške.** Biblioteke vraćaju `Result` s vlastitim tipom greške (`thiserror`). `unwrap()`,
`expect()` i `panic!` su dozvoljeni u testovima i u `og-cli`, ne u bibliotečnom kodu. Iznimka:
invarijanta koja je dokazano nemoguća — tada `expect()` s porukom koja objašnjava zašto.

**Perf.** Vrući put (generiranje poteza, evaluacija, rollout) nema alokacija po pozivu gdje se
to može izbjeći. Koristi bufere koji se ponovno koriste. Ali: prvo točno, pa tek onda brzo, i
nikad optimizacija bez benchmarka koji je pokazao problem.

**Unsafe.** Samo uz komentar koji dokazuje invarijantu. Za sada: očekivano samo u mmap pristupu
bearoff bazama.

---

## 5. Testovi

Za `og-core` i `og-bearoff`: **test prije implementacije.** Rubni slučajevi su tihi ubojice.

Svaki rubni slučaj iz Faze 1 ima imenovani test — ne smije nestati u generičkom fuzzu:

| Slučaj | Ime testa |
|---|---|
| Ulazak s bara | `entry_from_bar` |
| Nemogućnost ulaska s bara | `entry_from_bar_blocked` |
| Prisilno igranje većeg broja kad ide samo jedan | `forced_higher_die_when_only_one_playable` |
| Dublet s manje od četiri izvediva poteza | `doubles_with_fewer_than_four_moves` |
| Bearoff tek kad su svi kamenovi u home boardu | `bearoff_requires_all_checkers_home` |
| Bearoff s višeg broja bez kamena na točki | `bearoff_from_higher_die` |
| Pozicija bez ijednog legalnog poteza | `no_legal_moves` |

Ostalo:
- **Diferencijalni test protiv GNUbg-a** je mjerilo Faze 1: milijun nasumičnih pozicija × 21
  bacanje, skup legalnih poteza identičan. Fiksni seed, pad testa ispisuje poziciju i bacanje
  u obliku koji se može zalijepiti natrag u test.
- Regresijski testovi: svaki bug koji je prošao kroz testove dobiva svoj test prije popravka.
- Property testovi (`proptest`) su dobrodošli **uz** imenovane, ne umjesto njih.
- Benchmarke vodi od prvog dana. Regresija u brzini se kasnije teško lovi.

---

## 6. Tijek sesije

1. Pročitaj odjeljak 0 i relevantnu fazu u `OPENGAMMON.md`.
2. Reci što ćeš raditi i koji dio definicije "gotovo" to zatvara. Čekaj potvrdu za veće zahvate.
3. Testovi → implementacija → `cargo test` → `clippy` → `fmt`.
4. Male, zaokružene promjene. Radije tri commita koji rade nego jedan veliki koji možda radi.
5. Na kraju: ažuriraj odjeljak 0 i reci što ostaje otvoreno.

**Commit poruke:** `og-core: add bar entry move generation`. Prefiks je crate ili `docs`/`ci`.
Bez emojija, bez potpisa, bez spominjanja alata kojim je pisano.

---

## 7. Kad nešto nije jasno

- **Pravila backgammona:** provjeri protiv GNUbg-a (kao vanjski proces ili čitajući njegov kod
  radi razumijevanja), ne pretpostavljaj. Zapiši nalaz u `docs/rules-notes.md`.
- **Opseg ili prioritet:** pitaj. Ne proširuj opseg sam.
- **Dizajnerska odluka koja se kasnije teško mijenja** (reprezentacija pozicije, format na
  disku, javni API): predloži dvije opcije s posljedicama i čekaj odluku.
- Sve što je u `OPENGAMMON.md` §2 fiksirano — Rust, PyTorch, expectimax, rezidualni MLP,
  MIT/Apache — ne otvara se ponovno bez izričitog razgovora.

---

## 8. Što izričito ne raditi

- Ne preskačeš faze i ne pišeš "pripremu za kasnije" u trenutnoj fazi.
- Ne gradiš multiplayer, račune, matchmaking ni moderaciju — to je Tablea, zaseban projekt.
- Ne pozicioniraš projekt kao "bolji od XG-a" ni u kodu, ni u dokumentaciji, ni u README-u.
  Jedina ciljana tvrdnja je ona iz Faze 9, i to tek kad bude dokazana.
- Ne objavljuješ rezultat na referentnom skupu prije nego je skup i protokol javno objavljen.
- Ne dodaješ head-to-head mečeve kao mjerilo snage. Varijanca ih čini beskorisnima.
- Ne pišeš README koji obećava faze koje nisu gotove.

---

## 9. Odnos prema Tablei

`og-core`, `og-engine` i `og-rollout` se pišu kao biblioteke s čistim API-jem jer ih Tablea
kasnije uvlači kao ovisnost ili servis — **nikad kao kopirani kod**. Praktično: javni API ne
pretpostavlja CLI, datotečni sustav ni globalno stanje, i ne vuče ovisnosti specifične za web.

---

## 10. Rječnik (hrvatski → engleski u kodu)

| HR | EN |
|---|---|
| kamen | checker |
| točka | point |
| bacanje | roll |
| kockice | dice |
| kocka (za udvostručavanje) | cube |
| dublet | doubles |
| ploča / kuća | board / home board |
| bar | bar |
| iznošenje | bear off / bearoff |
| pip razlika | pip count |
| prime | prime |
| shot | shot |
| meč | match |
| potez | move / play |
| pozicija | position |

Pojmove koji nemaju ustaljen hrvatski prijevod (equity, cubeful, cubeless, gammon, backgammon,
Crawford, beaver, market losers) ne prevodi — ni u kodu, ni u razgovoru.
