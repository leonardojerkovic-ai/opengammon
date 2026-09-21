# Prior art

Bilješke o drugim backgammon motorima i o tome što OpenGammon iz njih preuzima.

**Pravilo za ovaj dokument:** samo ideje, algoritmi i objavljeni rezultati, prepričani vlastitim
riječima. Ni jedan redak koda, ni jedna datoteka s težinama mreže i ni jedan skup podataka ne
preuzima se iz motora navedenih ovdje. Čitanje tuđe *dokumentacije* da bi se razumjela ideja je u
redu; čitanje tuđeg izvornog koda dok se piše ekvivalentni dio OpenGammona nije.

---

## Open Sage (bgsage)

- Repozitorij: https://github.com/markbgsage/bgsage
- Licenca: **AGPL-3.0**. Strože od GNUbg-ovog GPLv3: mrežna klauzula znači da bi web servis ili
  API izgrađen na preuzetom kodu morao objaviti cijeli svoj izvorni kod pod AGPL-om. To je u
  izravnom sukobu s MIT/Apache-2.0 licencom OpenGammona i s planovima za Tableu.
- Tretman je isti kao za GNUbg, samo stroži: smije se pokretati kao vanjski proces radi
  validacije; nikad se ne linka, ne kopira i ne koristi kao izvor labela za trening.
- Pregledano isključivo iz projektne dokumentacije (`MULTI-PLY.md`, `ROLLOUT.md`,
  `XG_COMPARISON.md`, `MODEL_BENCHMARKS.md`), rujan 2026.

### Što je to

C++/Python motor s neuronskom evaluacijom, N-ply pretragom, skraćenim i punim rolloutovima s
redukcijom varijance te cubeful evaluacijom. Autori izvještavaju da je jači od XG-a na većini
usporedivih razina evaluacije za poteze, a otprilike izjednačen s XG-om u cube odlukama.

### Nalazi po fazama

#### Faza 3 — Rolloutovi

1. **Redukcija varijance mora koristiti isto pravilo odabira poteza kojim trial stvarno igra.**
   Član sreće je `stvarno − očekivano`, gdje `očekivano` prosječi po svih 21 bacanja. Ako se
   potez odabran za svako bacanje unutar `očekivano` razlikuje od poteza koji trial doista
   odigra (druga dubina, drugo razrješenje izjednačenih kandidata, cubeless umjesto cubeful
   rangiranja, mreža umjesto egzaktne bearoff vrijednosti), član sreće dobiva prosjek različit
   od nule. Ta pristranost **ne** pada s brojem triala i može gurnuti vjerojatnosti izvan
   [0, 1].
   → Traži imenovani test u `og-rollout`.
2. **Sreću mjeri na 1-ply bez obzira na dubinu odlučivanja.** Obje strane razlike koriste isti
   jeftini evaluator, pa im se pristranosti poništavaju. Dublja pretraga služi samo za odabir
   poteza. Ušteda je reda veličine.
3. **Stratificirane kockice.** Broj triala kao višekratnik 36 (72, 360, 1296 = 36²), uz
   hijerarhijsku shemu permutacija, tako da je prvo bacanje — ili prva dva — pokriveno točno.
   Sreća na prvom potezu tada zbraja u točno nulu i ne treba je ni računati.
4. **Deklarirana dubina mora stvarno vrijediti.** Postavka tipa "nakon poteza N spusti se na
   1-ply" učinila je da su njihovi nominalno 3-ply rolloutovi bili zapravo 1-ply. Otkriveno je
   tek usporedbom s XG-om.
   → Test da je konfigurirana dubina odlučivanja doista dubina korištena kroz cijeli trial.
5. **Cube odluke unutar triala ne spuštaju se na plići ply kasno u trialu**, za razliku od
   odluka o potezima. Ako se take/pass odlučuje pliće od evaluacije koja boduje trial,
   protivnik prima kocke koje bi dosljedna evaluacija pasala.
6. **Predmemoriranje prva dva poluteza.** Svaki trial kreće iz iste pozicije, pa se 21 odluka
   prvog poteza i 21×21 odluka drugog mogu izračunati jednom i dijeliti među svim trialima.

#### Faza 4 — Evaluacijski harness i referentni skup

1. **Adaptivni referentni skup u tri prolaza.** Odigraj partije na baznoj razini; ponovno
   procijeni skraćenim rolloutom svaku odluku čiji je razmak između najboljeg i drugog
   kandidata manji od 0.05; pusti puni rollout na svaku odluku koja je i dalje unutar 0.02, dok
   95% interval ne padne ispod 0.005 (ili dok se ne dosegne gornja granica broja triala).
   Performance Rating = prosječna greška equityja × 500, odvojeno za poteze i cube odluke te po
   tipu pozicije.
2. **Rollout nije neutralni sudac.** Na pozicijama gdje se dva motora ne slažu, rollout svakog
   motora sustavno daje pravo tom motoru. Posljedica za OpenGammon: referentni skup nastao
   isključivo iz vlastitih rolloutova strukturno je pristran u korist vlastitog motora.
   → Sporne pozicije moraju se suditi rolloutovima više od jednog motora, i to mora pisati u
   protokolu **prije** nego postoji ijedan rezultat.
3. **Benchmarkovi po obiteljima pozicija.** Odvojeni skupovi za deset klasičnih backgamea,
   containment partije, masivne backgame i "snake". Rijetke su u self-playu, povijesno su
   najslabija točka svakog motora, i ondje se pojavljuju najveće razlike među motorima.
   → Dobar predložak za stratifikaciju našeg referentnog skupa.
4. **Slaganje PR-a na stvarnim mečevima.** Ponovno analiziraj turnirske mečeve koji su već
   analizirani u XG-u i usporedi PR po igraču. Njihova prijavljena prosječna razlika je
   statistički nerazlučiva od nule.
   → Bitno za Fazu 7: igrači vjeruju analizi koja im daje PR na koji su navikli.

#### Faza 5 — Mreža

1. **Specijalizacija po planu igre mjerljivo pomaže.** Pozicije se svrstavaju u planove (čista
   trka, trka, napad, prime, sidro), zatim u parove *moj plan × protivnikov plan*, uz namjenske
   mreže za backgame. Svaki korak je poboljšao njihove benchmarkove. Naš spec trenutno ima tri
   mreže (kontakt / trka / bearoff).
2. **Veličina mreže naspram cijene pretrage.** Njihove mreže imaju jedan skriveni sloj od
   nekoliko stotina jedinica, a srodni kandidati se evaluiraju inkrementalno (potez mijenja
   samo 4–12 od ~244 ulaza, pa se ažuriraju samo ti stupci). Naš planirani rezidualni MLP
   (10–20M parametara) je oko 50× skuplji po evaluaciji i ne može koristiti taj trik. Kod
   expectimaxa na 3–4 ply to je velika razlika.
   → Izmjeriti prije nego se fiksira: prvo istrenirati malu jednoslojnu bazu, pa usporediti
   snagu **po jedinici vremena pretrage**, ne po evaluaciji.
3. **Postavi nemoguće ishode na nulu.** Nakon svake evaluacije postavi točno na nulu
   vjerojatnosti koje pozicija isključuje (gammon kad je gubitnik već iznio kamen, backgammon
   kad je kontakt prekinut i opasna zona prazna). Jeftino, uklanja šum.

#### Faza 9 — Cubeful istraživanje

Njihova cubeful evaluacija koristi Janowskijevu interpolaciju s fiksnom učinkovitošću kocke
(0.68 u kontaktu; formula po pip countu u trci). Njihov prijavljeni PR za cube odluke izjednačen
je s XG-ovim ili neznatno slabiji, dok im je PR za poteze bolji.

→ Cube odluke ostaju otvoren problem i za najjači otvoreni motor koji danas postoji. To
potkrepljuje tezu Faze 9 (učiti cubeful equity izravno umjesto rekonstrukcije preko
Janowskijeve formule), a ne je opovrgava.

### Pozicioniranje

Open Sage pokriva dobar dio onoga što §1.1 navodi kao nedostatak. Razlikovne prednosti
OpenGammona u odnosu na njega:

- dozvoljena licenca (MIT/Apache-2.0) — drugi mogu graditi i komercijalno;
- radi u pregledniku preko WASM-a, na mobitelu, bez instalacije;
- otvoreni `.ogn` format;
- analitički sloj (tipovi grešaka, statistika kroz mečeve, ponavljajući uzorci);
- istraživanje kocke iz Faze 9.

§1.1 ipak treba ažurirati prije Faze 8: tvrdnja "ni jedan ne nudi moderan analitički sloj" više
ne stoji u obliku u kojem je napisana, i prvo pitanje koje će stići sa zajednice bit će
"a zašto ne Sage?".

### Moguća suradnja

Projekti ciljaju različite stvari. Vrijedi kontaktirati autora prije Faze 4 oko dijeljenja
referentnih pozicija pod dozvoljenom licencom i međusobne validacije rezultata. Njihove
objavljene benchmark podatke ne koristiti bez izričitog dopuštenja, s obzirom na AGPL
repozitorij u kojem se nalaze.

---

## GNU Backgammon

- Licenca: GPLv3. Poziva se isključivo kao vanjski proces, iz testova i `og-eval`.
- Referenca za točnost generatora poteza (Faza 1) i za bearoff vrijednosti (Faza 2).
- Ima ugrađeni Python sloj — koristiti njega za strojno čitljiv izlaz umjesto parsiranja ASCII
  prikaza ploče.

## eXtreme Gammon

- Komercijalan, zatvoren, bez programskog sučelja. Usporedba ide preko njegove Batch Analysis
  funkcije i izvoza, ne programski.
- Referenca za validaciju u Fazi 4, nikad izvor labela za trening.
