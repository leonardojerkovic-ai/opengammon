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
  - **10k probni run gnubg diferencijalnog testa prošao čisto**: 210 000 usporedbi (10 000 pozicija × 21 bacanje), nula neslaganja, 4308 sekundi, bez pada. Time je hipoteza o rastu memorije 32-bitnog `gnubg-cli.exe` procesa preko desetaka tisuća `hint()` poziva **oborena**. Uzrok dva prijašnja pada ("gnubg-cli exited before answering a request", prazan stderr, na ~2000–3000 pozicija) bio je konkurentna `cargo build`/`cargo test` naredba usred runa koja je zamijenila testnu binarku (exit 127) — pravilo u §3 (nijedna druga `cargo` naredba dok dugi test vrti) to pokriva. Recikliranje gnubg sesije izbačeno iz plana — rješavalo bi problem koji ne postoji.
  - **Paralelizacija diferencijalnog runa gotova**: `random_self_play_positions_match_gnubg` sad dijeli uzorak na `OG_DIFF_WORKERS` kontinuiranih, nepreklapajućih raspona, svaki sa svojom `GnubgSession` (svoj `gnubg-cli` proces). Nastavljanje po workeru na `OG_DIFF_CHECKPOINT_DIR` (zadano `target/gnubg_diff_checkpoint`): svaki worker upisuje sljedeći neobrađeni globalni indeks nakon svake pozicije, pa isti poziv nakon prekida (pad, reboot, Ctrl-C) nastavlja odande, ne iznova. Neslaganje u bilo kojem workeru zaustavlja sve (provjera po poziciji, ne po bacanju) umjesto da ostali nastave trošiti sate GNUbg upita nakon što je run već pao. Provjereno ručno (40 pozicija/4 workera end-to-end, plus simulirani prekid nasred raspona jednog workera — nastavak je točan i ne ponavlja već potvrđene pozicije) i automatskim testom `chunk_bounds_partitions_without_gaps_or_overlap` za samu logiku dijeljenja raspona.
  - **Milijunski test ostaje neodrađen.** Preduvjeti (čist 10k run, paralelizacija) su sad zadovoljeni; ostaje sam run u punoj veličini.
  - Neistraženo, uočeno usput: `cargo test -p og-core --lib` (paralelno, debug profil) jednom je pao s exit `0xffffffff` dok je `self_play::tests::phase_distribution_of_the_diff_test_sample` radio usporedno s ostalim testovima. Sumnja da je uzrok duboka rekurzija u `collect_plies` je **opovrgnuta**: dubina je dokazano i izmjereno ograničena na točno 5 (dubleti), neovisno o faktoru grananja — vidi `docs/rules-notes.md`. Vjerojatniji uzrok: taj test je analiza (10 000 self-play partija po do 120 poteza), ne provjera, i pretežak je za jednu nit u debug-modu usporedno s ostalim testovima — sad označen `#[ignore]`. Nalaz i otvorena hipoteza u `docs/backlog.md`.
  - Ostaje do definicije "gotovo" Faze 1 (milijun pozicija × 21 bacanje, identičan skup):
    - sam milijunski run, preko noći, s `OG_DIFF_WORKERS` postavljenim blizu broja jezgri
    - **Stanje 2026-09-23: run pokrenut, ručno prekinut (Ctrl-C) na ~111 559/1 000 000 (~11.2%), checkpoint netaknut u `target/gnubg_diff_checkpoint/worker_*.txt`, nula neslaganja do prekida.** Nastavak isti poziv (svaki worker čita svoj checkpoint i nastavlja, ne iznova):
      ```powershell
      cd C:\Users\leona\Desktop\opengammon
      $env:OG_DIFF_SAMPLE_SIZE = '1000000'
      $env:OG_DIFF_WORKERS = '12'
      Start-Transcript -Path gnubg_million_run.log -Append
      cargo test -p og-core --release -- --ignored --exact gnubg_diff::random_self_play_positions_match_gnubg --nocapture
      ```
      Dok vrti: nijedna druga `cargo` naredba (vidi §3) — uključujući `og-bearoff`-ove GNUbg testove ispod.
- **Poznati dug:** WASM build provjera za `og-core` u CI-ju je dodana (`wasm` job u `.github/workflows/ci.yml`, `cargo build --target wasm32-unknown-unknown -p og-core`, zeleno) — pokupljena prije Faze 2 jer `og-bearoff` uvodi mmap pristup datotekama koji u WASM-u ne radi isto (nema datotečnog sustava u pregledniku). `og-bearoff` sad ima stvaran kod i lokalno se potvrđeno gradi čisto za `wasm32-unknown-unknown` (provjereno više puta tijekom sesije 2026-09-23), ali CI job to još ne provjerava — `wasm` job u `ci.yml` i dalje gradi samo `og-core`. Proširiti na `og-bearoff` ostaje otvoreno. Uz to, gornji neistraženi pad paralelnih testova.

- **Faza 2 (svjesno odstupanje od "faza po faza", dogovoreno s korisnikom 2026-09-23 — Faza 1 nije zatvorena):**
  - `crates/og-bearoff/src/combinatorial.rs`: generičko `rank`/`unrank` (const generic po broju točaka) za kodiranje rasporeda kamena u gust indeks; `count()` sam računa binomni koeficijent. Iscrpno testirano za pravi jednostrani oblik (6 točaka/15 kamena → 54 264 pozicije).
  - `og-core::Position::from_raw` promijenjen iz `#[cfg(test)]`-only nevalidirajućeg u javni, validirajući konstruktor (`Result<Self, PositionError>`, provjerava najviše 15 kamena po igraču, dopušta djelomične/jednostrane pozicije). Stari nevalidirajući oblik preživljava kao `from_raw_unchecked`, `pub(crate)`, `#[cfg(test)]`.
  - `crates/og-bearoff/src/one_sided.rs`: DP unatrag za jednostranu bazu. Po poziciji: `finish` (bacanja do zadnjeg kamena) i `first_off` (do prvog, za gammon) — **dva odvojena pravila odabira poteza**, ne jedno (otkriveno usporedbom s GNUbg-om, vidi `docs/rules-notes.md`). `first_off` se sprema samo za pozicije s `off == 0` (15 504 od 54 264) — GNUbg-ova "saving gammon" statistika je retroaktivna (trivijalna čim je bilo koji kamen već iznesen), pa nema smisla pamtiti je drugdje.
  - Validacija: iscrpni testovi (zbroj razdiobe = 1 za sve pozicije), Monte Carlo unakrsna provjera (dvije odvojene simulacije, jedna po pravilu), pa GNUbg usporedba — 10 ručno odabranih pozicija, zatim 300 nasumičnih, obje čiste (najveće odstupanje ~0.012 postotnih bodova, obično zaokruživanje).
  - `crates/og-bearoff/src/quantize.rs`: kvantizacija `f64` → `u16` (×65535) za kompaktan zapis. Zbroj ostaje točno 65535 u zapisu (zadnja vrijednost = ostatak, ne zaokružena zasebno; preljev iznad 65535 od nezavisnog zaokruživanja rješava se oduzimanjem od najveće vrijednosti). Iscrpno provjereno na cijeloj tablici, najveća greška ~6×10⁻⁵.
  - **Iscrpna GNUbg usporedba na kvantiziranim vrijednostima, svih 54 264 pozicije, odrađena 2026-09-24 (38 min, `gnubg_diff.rs::exhaustive_quantized_comparison_matches_gnubg`).** Usput otkriven i istražen near-tie fenomen u `finish` pravilu odabira poteza (razmak između najboljeg i drugog kandidata ponekad ispod 1e-4, čak i točno 0 negdje u tablici — vidi `docs/rules-notes.md`), pa je test prepravljen da ne stane na prvom promašaju nego prođe do kraja i skupi statistiku, s oznakom ima li pozicija koja promaši toleranciju *vlastitu* near-tie ili je odstupanje samo naslijeđeno od pretka u DP-u. Rezultat: **8 od 54 264 pozicije (0.015%) prelazi toleranciju od 0.05 postotnih bodova, najgore 0.278; svih 8 ima vlastitu near-tie, nula čisto naslijeđenih.** `first_off` gotovo savršen (najveće odstupanje 0.002, svih 15 504 `off==0` pozicija). **Korisnik potvrdio 2026-09-24: prihvaćeno kao dokumentirano ograničenje, ne istražuje se dalje.**
  - `crates/og-bearoff/src/disk.rs`: format na disku + mmap pristup. `BearoffData` radi isključivo nad `&[u8]` (bez I/O, bez `unsafe`, bez platform-ovisnosti) — isti kod radi za wasm32 i nativno. `memmap2` je nova ovisnost, ali `optional = true` iza `mmap` Cargo feature-a (nije default) — `disk::native::MappedBearoffFile` postoji samo uz taj feature; obična `cargo build --target wasm32-unknown-unknown -p og-bearoff` ga uopće ne povlači (provjereno, ne pretpostavljeno). Zaglavlje (20 B): magični broj, verzija, `points`/`max_checkers`, i **izmjerene** `max_finish_len`/`max_first_off_len` (31, 10) koje piše writer, ne konstante kojima čitač vjeruje. Fiksni zapisi (offset = `rank * record_len`, bez tablice offseta), eksplicitno little-endian svugdje. `first_off`-ov gusti pod-indeks ponovno koristi `combinatorial::rank` s `points-1` točaka. Iscrpno round-trippano (54 264 `finish` + 15 504 `first_off`) protiv tablice u memoriji.
  - **"Lookup ispod mikrosekunde" izmjereno (`disk.rs::finish_lookup_is_under_a_microsecond`, `Instant`, zagrijavanje pa mjerenje u nasumičnom redoslijedu, tri runa):** prosjek dosljedno 809-929 ns, min 200-300 ns — unutar kriterija. Najgori slučaj jako varira (151 µs / 198 µs / 2.05 ms) — OS raspoređivanje/stranica, ne trošak samog dizajna (izračun je O(1)), pa se tvrdi samo na prosjeku.
  - **Jednostrana bearoff baza (DP, GNUbg usporedba, kvantizacija, disk format, mmap, brzina) time je zaokružena.** Preostaje za kasnije u Fazi 2 (ne sada): dvostrana baza. Prije bilo čega novog: natrag na Fazu 1 (milijunski run, ~11% gotovo) — Faza 2 ostaje svjesno odstupanje dok se Faza 1 formalno ne zatvori.

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
