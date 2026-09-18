# OpenGammon — projektna specifikacija

> Ovaj dokument je izvor istine za projekt. Namijenjen je za rad kroz Claude Code.
> Prije svake sesije pročitaj relevantnu fazu. Ne preskači faze.

---

## 1. Što gradimo

**OpenGammon** je otvorena backgammon infrastruktura: motor, format, API i web alat za analizu.
**LeoBG** je ime motora unutar projekta.

Projekt se NE pozicionira kao "bolji od XG-a". Pozicionira se kao otvorena infrastruktura
koja zajednici nedostaje. Nadmašivanje XG-a u cube odlukama je istraživački cilj (Faza 9),
ne obećanje.

### 1.1 Motivacija

- eXtremeGammon (komercijalni standard) nije se razvijao od 2013.
- GNU Backgammon je open source, ali arhitektura je iz kasnih 1990-ih.
- Ni jedan ne nudi: web, mobilni pristup, API, otvoreni format, moderan analitički sloj.
- Cubeful odluke u match playu su mjerljivo slabije od odluka o potezima kod svih postojećih botova.

### 1.2 Opseg

U opsegu:
- analiza mečeva (primarno)
- igra protiv bota (sekundarno, kao demo i ulaz u analizu)
- otvoreni format i javni API

Izvan opsega:
- multiplayer, računi, matchmaking, moderacija, borba protiv varanja
- to pripada zasebnom projektu (Tablea)

---

## 2. Tehničke odluke (fiksirane)

| Područje | Odluka | Obrazloženje |
|---|---|---|
| Jezgra | Rust | Nativno za trening, WASM za preglednik, isti kod |
| Trening | PyTorch | Izvoz u ONNX ili vlastiti kvantizirani format |
| Pretraga | Expectimax, ne MCTS | 21 ishod bacanja je malo dovoljno za egzaktno nabrajanje |
| Arhitektura mreže | Rezidualni MLP | Pozicija nema sekvencijalnu strukturu; transformer nije opravdan |
| Licenca | MIT ili Apache 2.0 | GPL bi zatvorio integracije; želimo da drugi grade na tome |
| GNUbg | Samo kao vanjski proces | Nikad kopiran kod — GPLv3 bi se prenio na nas |
| Bearoff baze | Izračunate same | Egzaktne su, algoritam je pravocrtan, izbjegavamo licencu |

### 2.1 Ključno upozorenje o treningu

**Nikad ne treniraj na GNUbg ili XG evaluacijama.** Ako su labele njihove, plafon je njihova
razina. Njihovi rolloutovi se koriste isključivo za **validaciju**, nikad za učenje.

---

## 3. Struktura repozitorija

```
opengammon/
├── crates/
│   ├── og-core/        # pozicija, pravila, generiranje poteza
│   ├── og-bearoff/     # izračun i lookup bearoff baza
│   ├── og-engine/      # evaluacija, expectimax pretraga, inferencija mreže
│   ├── og-rollout/     # rollout engine s redukcijom varijance
│   ├── og-formats/     # parseri: .mat, XG, .sgf; spec i I/O za .ogn
│   ├── og-eval/        # evaluacijski harness, referentni skup
│   ├── og-wasm/        # WASM bindingi za web
│   └── og-cli/         # komandna linija: leobg
├── training/           # Python: PyTorch trening, self-play orkestracija
├── web/                # web sučelje
├── bench/              # referentni skup pozicija, protokol
├── docs/
│   ├── ogn-spec.md     # specifikacija formata
│   └── eval-protocol.md
└── CLAUDE.md           # upute za Claude Code sesije
```

---

## 4. Faze

Svaka faza ima definiciju "gotovo". Ne kreće se dalje dok nije zatvorena.

### Faza 0 — Temelji (1–2 tjedna)

Rust workspace, CI, licenca, CONTRIBUTING, kostur svih crateova.

**Gotovo kad:** CI vrti testove na svaki push; `cargo test` prolazi na praznom kosturu.

---

### Faza 1 — Move generator (3–4 tjedna) ← KRITIČNA

Reprezentacija pozicije i generiranje svih legalnih poteza.

Rubni slučajevi koji se MORAJU pokriti:
- ulazak s bara (i nemogućnost ulaska)
- prisilno igranje većeg broja kad se može odigrati samo jedan
- dubleti gdje je izvedivo manje od četiri poteza
- bearoff samo kad su svi kamenovi u home boardu
- bearoff s višeg broja kad nema kamena na točnoj točki
- pozicije bez ijednog legalnog poteza

**Gotovo kad:** na milijun nasumičnih pozicija × svih 21 bacanja, skup legalnih poteza je
**identičan** GNUbg-ovom. Ne 99.99% — identičan.

> Tolerancija je nula. Greške u generatoru su tihe i kvare sve iznad. Ovaj test hvata i
> greške modela koji piše kod i greške u razumijevanju pravila.

---

### Faza 2 — Bearoff baze (2 tjedna)

Jednostrana baza (15 kamena, 6 točaka) dinamičkim programiranjem unatrag.
Dvostrana baza ograničena na razumnu dubinu — pazi na veličinu na disku.
Kompresija, mmap pristup.

**Gotovo kad:** vrijednosti se poklapaju s GNUbg bazama na uzorku; lookup ispod mikrosekunde.

---

### Faza 3 — Rollout engine (4–5 tjedana)

Redukcija varijance je obavezna, ne opcionalna:
- zajedničke sekvence kockica za sve kandidate (isti "svijet", različita odluka)
- antitetičke varijante
- luck adjustment po potezu
- **skraćeni rolloutovi**: 8–12 poluteza pa evaluacija mrežom umjesto igranja do kraja

> Skraćeni rolloutovi su razlika između izvedivog i neizvedivog projekta na malom budžetu.
> Ušteda je red veličine, gubitak preciznosti mali.

Paralelizacija po CPU jezgrama.

**Gotovo kad:** isti rollout s različitim seedovima daje rezultate unutar deklarirane greške;
izmjereno je koliko je varijanca smanjena u odnosu na naivni rollout.

---

### Faza 4 — Evaluacijski harness i referentni skup (3 tjedna)

- nekoliko tisuća pozicija, stratificirano po fazama igre i tipovima
- posebno: pozicije gdje se XG i GNUbg razilaze (tu se zapravo dobiva ili gubi)
- mjerenje prosječnog gubitka equityja, **odvojeno za poteze i za cube odluke**
- automatska usporedba bilo koje dvije verzije motora

Head-to-head mečevi se NE koriste kao mjerilo — varijanca traži desetke tisuća mečeva za
razlikovanje botova koji se razlikuju za 0.005 equityja.

**Gotovo kad:** skup i protokol su **javno objavljeni, prije nego postoji ijedan vlastiti
rezultat.** Time se unaprijed odgovara na prigovor da su birani testovi koji odgovaraju.

---

### Faza 5 — Prva mreža (6–10 tjedana)

**Ulazna reprezentacija** (hibrid, ne sirova ploča):
- one-hot po točkama, Tesaurov stil: za svaku točku 4 neurona po igraču (1, 2, 3, n>3 kamena)
- bar i off
- izvedene značajke: duljina primea, pip count i razlika, kontakt/nekontakt, broj shotova
  protivnika, raspodjela u home boardu
- kanal za kocku: vrijednost, vlasništvo
- kanal za meč: away_us, away_them, Crawford flag

**Arhitektura:** rezidualni MLP, 8–12 blokova širine 1024–2048 (~10–20M parametara).
Odvojene mreže po fazi igre: kontakt / trka / bearoff.

**Izlaz (tri glave):**
1. distribucijska glava ishoda (kvantili) → iz nje i equity i **volatilnost**
2. cubeful equity glava, uvjetovana stanjem kocke i rezultatom meča
3. policy glava P(potez | pozicija, bacanje) — samo za uređivanje kandidata u pretrazi

U fazi 5 koristi se samo cubeless dio. Glave 1 i 2 se aktiviraju u fazi 9.

**Trening:** TD(λ) self-play od nule, cubeless, epizoda = jedna partija.
Zatim iterativno produbljivanje rolloutovima vlastitog motora.

**Gotovo kad:** na referentnom skupu si unutar mjerljive blizine GNUbg-a u potezima.

---

### Faza 6 — Formati i uvoz (3 tjedna)

Parseri za `.mat`, XG formate, `.sgf` varijantu. Parsere piši sam; GNUbg kod čitaj samo da
shvatiš što format sadrži i gdje su rubni slučajevi.

Specifikacija `.ogn` (OpenGammon Notation): pozicija, kandidati, equity, cube odluke,
metapodaci o dubini analize. Objavi kao dokument.

> Ako se format primi, projekt postaje infrastruktura zajednice neovisno o snazi motora.

**Gotovo kad:** uvezeš tuđu arhivu od tisuću mečeva bez gubitka podataka i izvezeš natrag.

---

### Faza 7 — Web i analitički sloj (8–10 tjedana)

- WASM build motora, Web Worker
- ploča, uvoz meča, prikaz analize
- **radi na mobitelu bez instalacije**

Analitika koju XG nema — ovdje je stvarna prednost:
- greške klasificirane po **tipu**, ne samo po veličini
- statistika kroz stotine mečeva, po fazama i tipovima pozicija
- prepoznavanje ponavljajućih uzoraka kod pojedinog igrača
- objašnjenja na prirodnom jeziku iznad izlaza motora

> LLM nikad ne procjenjuje poziciju sam. Dobiva poziciju, kandidate i equity iz motora i
> samo prevodi u tekst.

Igra protiv bota: samo ploča, bot i gumb "analiziraj". Bez satova, turnira i povijesti.
Služi kao demo i kao prirodan ulaz u analizu.

Javni API.

**Gotovo kad:** stranac bez uputa uveze svoj meč i dobije korisnu analizu na mobitelu.

---

### Faza 8 — Javno izdanje

Otvoreni kod, dokumentacija, objava zajednici (BGonline, r/backgammon, klubovi).
Prije objave: tri-četiri jaka igrača koji su već testirali i mogu potvrditi.

> Mišljenje jakog igrača "ovo je u pravu tamo gdje XG griješi" vrijedi više od statistike
> koju sam objaviš.

---

### Faza 9 — Cubeful istraživanje (otvoren kraj)

Ovo je istraživački projekt, ne inženjerski. Nema rok i ne obećava se unaprijed.

- distribucijska glava (volatilnost → "market losers" direktno, ne posredno)
- cubeful equity naučen **direktno**, umjesto rekonstrukcije iz cubeless procjene preko
  Janowskijeve formule s cube life indexom
- meč-level self-play: epizoda je cijeli meč, akcije uključuju double/no-double, take/pass/beaver
- match equity tablica prestaje biti zaseban artefakt — postaje implicitna u mreži

Ciljana područja poznatih slabosti: 2-away/2-away, post-Crawford, gammon-go, volatilne
pozicije gdje Janowskijeva aproksimacija puca.

NE pokušavaj nadmašiti XG u potezima u kontaktnim pozicijama — vjerojatno je blizu plafona.

**Jedina tvrdnja koju ciljamo dokazati:** niži prosječni gubitak equityja u cube odlukama u
match playu od XG-a, na skupu objavljenom prije nego je rezultat bio poznat.

**Gotovo kad:** tvrdnja je dokazana ili opovrgnuta. Oboje je valjan ishod.

---

## 5. Hardver po fazama

| Faza | Što treba |
|---|---|
| 0–4 | Obični laptop. Više jezgri pomaže za rolloutove (8–16 udobno, 4 radi). Disk za dvostranu bearoff bazu. |
| 5 | Jedan GPU s 24GB (3090/4090/5090) + 16–32 CPU jezgre. Omjer je bitan: self-play je dominantno CPU posao, GPU samo evaluira. |
| Rolloutovi | Desetci tisuća CPU-sati kroz više generacija. Red veličine nekoliko tisuća do desetak tisuća eura na cloudu. Ovdje se traži sponzorstvo ili grant. |
| 9 | Višekratnik prethodnog. Ne planirati dok se ne vidi kako ide faza 5. |

**Ne kupovati ništa unaprijed.** Kad dođeš do faze 5, unajmi instancu na tjedan-dva i vidi
gdje je stvarno usko grlo prije nabave.

---

## 6. Rizici

| Rizik | Mitigacija |
|---|---|
| Greška u move generatoru | Test protiv GNUbg-a na milijun pozicija, tolerancija nula |
| Trening na tuđim labelama → plafon | Self-play od nule; tuđi rolloutovi samo za validaciju |
| Compute za rolloutove | Skraćeni rolloutovi; sponzorstvo; faze 0–8 su izvedive bez toga |
| Solo rad | Suradnik za ML dio prije faze 9 |
| Skepsa zajednice | Otvoreni kod od prvog dana; protokol objavljen prije rezultata; testeri rano |
| Faza 9 proguta projekt | Faze 0–8 su zasebna cjelina koja izlazi neovisno |

---

## 7. Upute za Claude Code sesije

- Radi fazu po fazu. Ne počinji sljedeću dok definicija "gotovo" nije zadovoljena.
- Za `og-core` i `og-bearoff` piši test prije implementacije. Rubni slučajevi su tihi ubojice.
- Svaki rubni slučaj iz Faze 1 mora imati imenovani test.
- Ne uvoditi ovisnost o GNUbg kodu ni u jednom crateu. GNUbg se poziva samo kao vanjski proces
  iz testova i `og-eval`.
- Benchmarke voditi od prvog dana; regresije u brzini su skupe kasnije.
- Kad nešto nije jasno u pravilima backgammona, provjeri protiv GNUbg-a umjesto pretpostavke.

---

## 8. Odnos prema Tablei

Motor iz OpenGammona je ono što će Tablea trebati za backgammon: bota, analizu odigranih
partija, eventualno detekciju varanja. Zato ga od početka pisati kao biblioteku s čistim
API-jem — Tablea ga kasnije uvuče kao dependency ili servis, nikad kao kopirani kod.

Redoslijed je OpenGammon → Tablea, ne obrnuto: backgammon zajednica koju OpenGammon dosegne
je prva grupa korisnika za backgammon dio Tablee.
