(() => {
  "use strict";

  const ukrainian = {
    skip: "Перейти до вмісту",
    navWhy: "Навіщо Upyr",
    navExperience: "Огляд",
    navHome: "Головна",
    navHow: "Як це працює",
    navFeatures: "Можливості",
    navPrivacy: "Приватність",
    navPlatforms: "Платформи",
    navStory: "Історія створення",
    heroEyebrow: "Публічна попередня версія · v0.3.0",
    heroTitle: "Розкладка може помилятися. Ваші слова — ні.",
    heroLead: "Upyr миттєво відновлює англійський та український текст, набраний у неправильній розкладці. Нативна швидкість Rust, локальні рішення, жодного стеження.",
    downloadMac: "Відкрити прев’ю macOS",
    viewSource: "Переглянути код",
    releaseNote: "Універсальний для Apple Silicon та Intel · macOS 11+ · поточне прев’ю не перевірене Apple",
    localOnly: "Лише локально",
    youTyped: "Ви набрали",
    physicalRow: "Ті самі фізичні клавіші",
    decisionTitle: "Впевнена відповідність",
    decisionCopy: "замінено за мілісекунди",
    trustNetwork: "Без мережі під час роботи",
    trustAccount: "Без облікового запису",
    trustTracking: "Без телеметрії",
    trustSource: "Відкритий код · MIT",
    whyKicker: "Не втрачайте думку",
    whyTitle: "Виправте розкладку. Збережіть хід думок.",
    whyLead: "Невеликий нативний інструмент проти одного конкретного відволікання — зроблений так, щоб зникати після виконаної роботи.",
    featureAutoTitle: "Автоматично, коли є впевненість",
    featureAutoCopy: "За бажанням Upyr виправляє текст на межі слова. Обережна політика не чіпає правильні слова, імена, URL-адреси та технічні токени.",
    featureManualTitle: "Вручну, коли вирішуєте ви",
    featureManualCopy: "Виправте виділення або попереднє слово сполученням клавіш. Початково Upyr намагається відновити джерело вводу та підтримувані формати буфера обміну.",
    featureKeysTitle: "Правда фізичних клавіш",
    featureKeysCopy: "Upyr будує мапінг зі встановлених розкладок і розуміє Shift, пунктуацію, апострофи, ї, є, ґ та швидкий змішаний ввід.",
    featureFeedbackTitle: "Ненав’язливий відгук",
    featureFeedbackCopy: "Необов’язкові прапорці мови та три локальні саундпаки додають короткий звуковий відгук. Кожен звук події та клавіші синтезується на пристрої; записи й готові аудіофайли не постачаються.",
    experienceKicker: "Побачте. Послухайте.",
    experienceTitle: "Нативні контролі та відгук, який налаштовуєте ви.",
    experienceLead: "Пошук у налаштуваннях macOS робить автоматизацію, сполучення клавіш, мовні прапорці та кожну звукову подію зрозумілими й необов’язковими.",
    settingsCaption: "Нативні налаштування AppKit у macOS",
    settingsCaptionDetail: "Пошук у вкладках, окремі контролі подій, попередній перегляд і одна зрозуміла дія «Save».",
    settingsExpand: "Відкрити повний розмір",
    soundLocalLabel: "Генерується наживо",
    soundTitle: "Спробуйте саундпаки",
    soundVolume: "Гучність прев’ю",
    soundLead: "Короткі прев’ю, які браузер генерує за мотивами локального синтезатора застосунку на Rust.",
    soundOriginalCopy: "М’який стриманий сигнал перетворення.",
    soundArcadeCopy: "Яскраве коротке ретро-підтвердження.",
    soundAnimeCopy: "Синтезована голосна реакція для Enter — без запису голосу.",
    soundPlay: "Відтворити",
    soundReady: "Готово · нічого не відтворюється автоматично",
    soundPrivacy: "Генерується локально у вашому браузері. Без аудіофайлів, мікрофона, мережі чи стеження.",
    howKicker: "Під капотом",
    howTitle: "Крихітний локальний шлях рішення",
    howLead: "Без сервісу перекладу та без генеративної моделі. Upyr перемаплює точні фізичні клавіші, а потім перевіряє, чи варіант в іншій розкладці справді правдоподібніший.",
    stepCaptureTitle: "Зчитати позиції",
    stepCaptureCopy: "Нативний перехоплювач тримає в пам’яті лише обмежений префікс поточної межі вводу.",
    stepMapTitle: "Перемапити клавіші",
    stepMapCopy: "Встановлені джерела вводу EN та УК утворюють варіант в іншій розкладці разом із регістром і пунктуацією.",
    stepScoreTitle: "Оцінити свідчення",
    stepScoreCopy: "Словники, захист технічних токенів, винятки та підписаний індекс символьних n-грам порівнюють обидва прочитання.",
    stepApplyTitle: "Застосувати обережно",
    stepApplyCopy: "Текст і джерело вводу змінюються лише тоді, коли варіант перевищує обраний поріг упевненості.",
    modelLabel: "ПІДПИСАНІ N-ГРАМИ v1",
    modelTitle: "Символьні шаблони в компактному форматі",
    modelCopy: "Однакові за розміром англійський та український новинні корпуси фільтруються за абеткою й розбиваються на послідовності з 2–5 символів. Кожна збережена послідовність стає запакованим ключем та одним байтом упевненості: від’ємним для англійської, додатним для української.",
    modelPrivacy: "Застосунок не містить речень корпусу чи таблиці частот слів — лише 173 964 записи n-грам приблизно у 2,8 МіБ. Пошук локальний, детермінований і миттєвий.",
    modelEval: "Переглянути зафіксовану оцінку",
    ngramLiveLabel: "ЛОКАЛЬНИЙ РОЗБІР",
    ngramDemoTitle: "Простежте одне виправлення",
    ngramReplay: "Повторити розбір",
    ngramMapTitle: "Зіставити фізичні клавіші",
    ngramMapCopy: "Ті самі шість позицій клавіш утворюють варіант в українській розкладці.",
    ngramSplitTitle: "Витягти перекривні n-грами",
    ngramSplitCopy: "Додати межі слова й переглянути кожне вікно з 2–5 символів. Довші n-грами мають більшу вагу.",
    ngramCount: "22 перекривні n-грами",
    ngramCompareTitle: "Порівняти локальні свідчення",
    ngramCompareCopy: "Запаковані записи знаходяться бінарним пошуком; довші n-грами отримують більшу вагу перед нормалізацією суми.",
    ngramCoverageLabel: "Покриття моделі · не ймовірність",
    ngramDecideTitle: "Ухвалити захищене рішення",
    ngramDecideCopy: "«привіт» є у словнику; «ghbdsn» — ні. Модель погоджується, а політика дозволяє виправлення.",
    ngramUnknown: "невідоме",
    ngramKnown: "відоме слово",
    ngramCorrect: "ВИПРАВИТИ",
    privacyKicker: "Приватність — це архітектура",
    privacyTitle: "Ваш набір тексту не має залишати Mac.",
    privacyLead: "Upyr не має системи облікових записів, хмарного інференсу, аналітичного SDK, телеметрії, звітів про збої, реклами чи автоматичних запитів оновлення.",
    securityPolicy: "Безпека та сповіщення",
    privacyMemoryTitle: "Короткочасна пам’ять",
    privacyMemoryCopy: "У пам’яті залишається лише обмежений поточний префікс. Межі та скидання видаляють його; набраний текст не записується в журнали чи на диск.",
    privacyClipboardTitle: "Захищений буфер обміну",
    privacyClipboardCopy: "Коли відновлення ввімкнене, Upyr намагається повернути підтримувані формати та повідомляє про помилки платформи. У macOS тимчасові дані позначені як приховані, а Copy перевіряється нативним лічильником змін.",
    privacyOptTitle: "Автоматизація за бажанням",
    privacyOptCopy: "Автоматичне виправлення, жести модифікаторів, звуки клавіш та прапорець мови початково вимкнені. Звуки отримують лише категорію фізичної клавіші й не зберігають набрані символи.",
    privacyAuditTitle: "Перевірювані заходи",
    privacyAuditCopy: "Відкритий код, відтворюване створення моделі, звіти RustSec, CodeQL, перевірка залежностей, тести та smoke-перевірки пакунків роблять твердження придатними до аудиту.",
    evidenceNote: "Інструменти безпеки зменшують ризик, але не можуть довести повну відсутність вразливостей. Генератор корпусу — єдиний навмисний завантажувач, і він працює лише після команди розробника. Десктопний та WASM-код не реалізують мережевого клієнта.",
    platformKicker: "Доставка",
    platformTitle: "Спочатку macOS. Одне портативне ядро.",
    platformLead: "Спільне ядро Rust уже працює на нативних десктопах і у WebAssembly. Продуктова якість свідомо доводиться платформа за платформою.",
    statusPrimary: "ОСНОВНА",
    macCopy: "Універсальний застосунок для Apple Silicon та Intel, нативні налаштування AppKit, мапінг джерел вводу, підтримка знімка й відновлення багатого буфера, відгук і запуск при вході.",
    macDownload: "Відкрити релізи",
    statusPreview: "ПРЕВ’Ю",
    windowsCopy: "Трей-застосунок, мапінг розкладки, підтримка знімка й відновлення буфера, інсталятор, портативний ZIP, CI-перевірки та доступність налаштувань через AccessKit.",
    statusPreview2: "ПРЕВ’Ю",
    linuxCopy: "XKB-мапінг, трей StatusNotifierItem, сповіщення про зміну розкладки, пакунки DEB і tar та доступність налаштувань через AccessKit. Глобальний ввід нативного Wayland очікує дизайну через портали.",
    statusEngine: "ЯДРО",
    wasmCopy: "Незалежне від DOM ядро й згенеровані контракти TypeScript готові. Далі — браузерний адаптер, демо та npm-пакунок.",
    aboutKicker: "Про назву",
    aboutTitle: "Створений перекидатися.",
    aboutCopy: "Назва Upyr походить від українського «упир» — перевертня, того, хто перекидається. Застосунок робить таке саме невелике перетворення з текстом у неправильній розкладці.",
    inspirationCopy: "Це незалежна реалізація на Rust, натхненна Punto Switcher і TolikPylypchuk/KeyboardSwitch, із вужчим фокусом на EN–UK та локальною архітектурою.",
    mitLicense: "Ліцензія MIT",
    downloadKicker: "Публічна попередня версія",
    downloadTitle: "Продовжуйте набирати. Решту перекине Upyr.",
    downloadCopy: "Якщо macOS повідомляє, що Apple не може перевірити Upyr, ви використовуєте поточне прев’ю з ad-hoc підписом без нотаризації. В Upyr немає автоматичного оновлювача; перевіряйте Releases для майбутніх нотаризованих збірок або збирайте з коду.",
    downloadButton: "Відкрити релізи",
    reportIssue: "Повідомити про проблему",
    footerTagline: "Набирайте те, що мали на увазі.",
    footerSource: "Код",
    footerReleases: "Релізи",
    footerIssues: "Проблеми",
    footerSecurity: "Безпека",
    footerStory: "Історія створення",
    footerMeta: "Без cookies · Без аналітики · Без зовнішніх ресурсів",
    homeLabel: "Головна сторінка Upyr",
    navLabel: "Основна навігація",
    languageLabel: "Обрати мову",
    githubLabel: "Upyr на GitHub",
    demoLabel: "Приклади перетворення Upyr",
    principlesLabel: "Принципи продукту",
    settingsScreenshotAlt: "Налаштування Upyr у macOS із контролями мовного індикатора та звукового відгуку",
    ngramKeyPairsLabel: "g відповідає п, h — р, b — и, d — в, s — і, n — т",
    ngramTokensLabel: "Перекривні n-грами довжиною від двох до п’яти символів",
    ngramRecordsLabel: "Приклади записів n-грам зі знаком",
    ngramReplayLabel: "Повторити розбір",
    settingsScreenshotLinkLabel: "Відкрити повнорозмірний знімок налаштувань Upyr",
    ngramStepsLabel: "Кроки розбору n-грам",
    ngramStep1Label: "Крок 1: зіставити фізичні клавіші",
    ngramStep2Label: "Крок 2: витягти перекривні n-грами",
    ngramStep3Label: "Крок 3: порівняти локальні свідчення",
    ngramStep4Label: "Крок 4: ухвалити захищене рішення"
  };

  // Per-page metadata: the English base is read from each page's own head, and
  // the Ukrainian strings come from data-* attributes on <html>. This keeps every
  // page's title/description its own, across the multi-page site.
  const metaContent = (selector) => {
    const node = document.querySelector(selector);
    return node ? node.getAttribute("content") || "" : "";
  };
  const pageData = document.documentElement.dataset;
  const englishMetadata = {
    title: document.title,
    description: metaContent('meta[name="description"]'),
    socialTitle: metaContent('meta[property="og:title"]') || document.title,
    socialDescription: metaContent('meta[property="og:description"]')
  };
  const ukrainianMetadata = {
    title: pageData.titleUk || englishMetadata.title,
    description: pageData.descriptionUk || englishMetadata.description,
    socialTitle: pageData.titleUk || englishMetadata.socialTitle,
    socialDescription: pageData.socialUk || englishMetadata.socialDescription
  };

  document.documentElement.classList.remove("no-js");
  document.documentElement.classList.add("js");

  const englishText = new Map();
  const textNodes = [...document.querySelectorAll("[data-i18n]")];
  const ariaNodes = [...document.querySelectorAll("[data-i18n-aria]")];
  const altNodes = [...document.querySelectorAll("[data-i18n-alt]")];

  for (const node of textNodes) {
    englishText.set(node, node.textContent.trim());
  }

  const englishAria = new Map();
  for (const node of ariaNodes) {
    englishAria.set(node, node.getAttribute("aria-label") || "");
  }

  const englishAlt = new Map();
  for (const node of altNodes) {
    englishAlt.set(node, node.getAttribute("alt") || "");
  }

  const liveMessages = {
    en: {
      soundReady: "Ready · nothing plays automatically",
      soundPlaying: (name) => `Playing ${name} preview`,
      soundFinished: (name) => `${name} preview finished`,
      soundUnavailable: "Audio preview is unavailable in this browser",
      soundError: "The preview could not be played",
      traceComplete: "Trace complete: dictionary, policy, and model evidence agree on привіт."
    },
    uk: {
      soundReady: "Готово · нічого не відтворюється автоматично",
      soundPlaying: (name) => `Відтворюється прев’ю ${name}`,
      soundFinished: (name) => `Прев’ю ${name} завершено`,
      soundUnavailable: "Аудіопрев’ю недоступне в цьому браузері",
      soundError: "Не вдалося відтворити прев’ю",
      traceComplete: "Розбір завершено: словник, політика й модельні свідчення погоджуються щодо «привіт»."
    }
  };

  const soundPackNames = {
    original: { en: "Original", uk: "Original" },
    arcade: { en: "Arcade", uk: "Arcade" },
    anime: { en: "Anime Reactions", uk: "Anime-реакцій" }
  };

  let currentLanguage = "en";
  let soundStatusState = { kind: "soundReady", pack: null };
  let traceStatusComplete = false;

  function setMetadata(metadata) {
    document.title = metadata.title;
    const description = document.querySelector('meta[name="description"]');
    const socialTitle = document.querySelector('meta[property="og:title"]');
    const socialDescription = document.querySelector('meta[property="og:description"]');
    if (description) description.content = metadata.description;
    if (socialTitle) socialTitle.content = metadata.socialTitle;
    if (socialDescription) socialDescription.content = metadata.socialDescription;
  }

  function setLanguage(language, updateAddress) {
    const isUkrainian = language === "uk";
    currentLanguage = isUkrainian ? "uk" : "en";
    document.documentElement.lang = isUkrainian ? "uk" : "en";

    for (const node of textNodes) {
      const key = node.dataset.i18n;
      const translation = ukrainian[key];
      node.textContent = isUkrainian && typeof translation === "string" ? translation : englishText.get(node);
    }

    for (const node of ariaNodes) {
      const key = node.dataset.i18nAria;
      const translation = ukrainian[key];
      node.setAttribute("aria-label", isUkrainian && typeof translation === "string" ? translation : englishAria.get(node));
    }

    for (const node of altNodes) {
      const key = node.dataset.i18nAlt;
      const translation = ukrainian[key];
      node.setAttribute("alt", isUkrainian && typeof translation === "string" ? translation : englishAlt.get(node));
    }

    for (const button of document.querySelectorAll("[data-language]")) {
      const selected = button.dataset.language === language;
      button.classList.toggle("is-active", selected);
      button.setAttribute("aria-pressed", String(selected));
    }

    setMetadata(isUkrainian ? ukrainianMetadata : englishMetadata);
    renderSoundStatus();
    renderTraceStatus();

    if (updateAddress) {
      const url = new URL(window.location.href);
      if (isUkrainian) {
        url.searchParams.set("lang", "uk");
      } else {
        url.searchParams.delete("lang");
      }
      window.history.replaceState(null, "", `${url.pathname}${url.search}${url.hash}`);
    }
  }

  function renderSoundStatus() {
    const status = document.querySelector("#sound-preview-status");
    if (!status) return;
    const messages = liveMessages[currentLanguage];
    const message = messages[soundStatusState.kind];
    if (typeof message === "function" && soundStatusState.pack) {
      const name = soundPackNames[soundStatusState.pack]?.[currentLanguage] || soundStatusState.pack;
      status.textContent = message(name);
    } else if (typeof message === "string") {
      status.textContent = message;
    }
  }

  function setSoundStatus(kind, pack = null) {
    soundStatusState = { kind, pack };
    renderSoundStatus();
  }

  function renderTraceStatus() {
    const announcer = document.querySelector(".ngram-announcer");
    if (announcer) {
      announcer.textContent = traceStatusComplete ? liveMessages[currentLanguage].traceComplete : "";
    }
  }

  const volumeInput = document.querySelector("#sound-preview-volume");
  const volumeOutput = document.querySelector('output[for="sound-preview-volume"]');
  const soundButtons = [...document.querySelectorAll("[data-sound-preview]")];
  const AudioContextConstructor = window.AudioContext || window.webkitAudioContext;
  let audioContext = null;
  let activePreview = null;
  let playbackSerial = 0;

  function previewVolume() {
    return Math.min(1, Math.max(0, Number(volumeInput?.value || 0) / 100));
  }

  function masterLevel() {
    return previewVolume() * 0.26;
  }

  function updateVolume() {
    const value = Math.round(previewVolume() * 100);
    if (volumeOutput) volumeOutput.textContent = `${value}%`;
    if (activePreview && audioContext) {
      activePreview.master.gain.setTargetAtTime(masterLevel(), audioContext.currentTime, 0.012);
    }
  }

  function getAudioContext() {
    if (!AudioContextConstructor) return null;
    if (!audioContext || audioContext.state === "closed") {
      audioContext = new AudioContextConstructor({ latencyHint: "interactive" });
    }
    return audioContext;
  }

  function createAudioOutput(context) {
    const bus = context.createGain();
    const compressor = context.createDynamicsCompressor();
    const master = context.createGain();
    compressor.threshold.value = -24;
    compressor.knee.value = 10;
    compressor.ratio.value = 10;
    compressor.attack.value = 0.003;
    compressor.release.value = 0.08;
    master.gain.value = masterLevel();
    bus.connect(compressor);
    compressor.connect(master);
    master.connect(context.destination);
    return { bus, master, nodes: [bus, compressor, master] };
  }

  function applyEnvelope(parameter, start, end, peak, attack = 0.007, release = 0.035) {
    parameter.setValueAtTime(0.0001, start);
    parameter.exponentialRampToValueAtTime(peak, start + attack);
    parameter.setValueAtTime(peak, Math.max(start + attack, end - release));
    parameter.exponentialRampToValueAtTime(0.0001, end);
  }

  function scheduleTone(context, bus, sources, nodes, options) {
    const oscillator = context.createOscillator();
    const filter = context.createBiquadFilter();
    const gain = context.createGain();
    oscillator.type = options.type;
    oscillator.frequency.setValueAtTime(options.from, options.start);
    oscillator.frequency.exponentialRampToValueAtTime(options.to, options.end);
    filter.type = "lowpass";
    filter.frequency.value = options.cutoff;
    filter.Q.value = 0.72;
    applyEnvelope(gain.gain, options.start, options.end, options.gain, options.attack, options.release);
    oscillator.connect(filter);
    filter.connect(gain);
    gain.connect(bus);
    oscillator.start(options.start);
    oscillator.stop(options.end + 0.01);
    sources.push(oscillator);
    nodes.push(filter, gain);
  }

  function scheduleOriginal(context, bus, start, sources, nodes) {
    const end = start + 0.165;
    scheduleTone(context, bus, sources, nodes, {
      type: "triangle", from: 760, to: 1190, start, end, cutoff: 4800, gain: 0.48, attack: 0.006, release: 0.042
    });
    scheduleTone(context, bus, sources, nodes, {
      type: "sine", from: 1130, to: 1640, start: start + 0.018, end, cutoff: 5600, gain: 0.17, attack: 0.005, release: 0.05
    });
    return end;
  }

  function scheduleArcade(context, bus, start, sources, nodes) {
    const firstEnd = start + 0.065;
    const secondStart = start + 0.058;
    const end = secondStart + 0.088;
    scheduleTone(context, bus, sources, nodes, {
      type: "square", from: 870, to: 1050, start, end: firstEnd, cutoff: 6800, gain: 0.2, attack: 0.004, release: 0.018
    });
    scheduleTone(context, bus, sources, nodes, {
      type: "triangle", from: 1170, to: 1510, start: secondStart, end, cutoff: 6800, gain: 0.38, attack: 0.004, release: 0.028
    });
    return end;
  }

  function scheduleAnime(context, bus, start, sources, nodes) {
    scheduleTone(context, bus, sources, nodes, {
      type: "sine", from: 720, to: 430, start, end: start + 0.028, cutoff: 4200, gain: 0.12, attack: 0.003, release: 0.012
    });

    const voiceStart = start + 0.022;
    const end = voiceStart + 0.21;
    const voice = context.createOscillator();
    const vibrato = context.createOscillator();
    const vibratoDepth = context.createGain();
    voice.type = "sawtooth";
    voice.frequency.setValueAtTime(195, voiceStart);
    voice.frequency.exponentialRampToValueAtTime(229, end);
    vibrato.type = "sine";
    vibrato.frequency.value = 5.8;
    vibratoDepth.gain.value = 4.2;
    vibrato.connect(vibratoDepth);
    vibratoDepth.connect(voice.frequency);

    const formants = [
      { frequency: 520, bandwidth: 95, gain: 0.11 },
      { frequency: 900, bandwidth: 125, gain: 0.066 },
      { frequency: 2520, bandwidth: 230, gain: 0.02 }
    ];
    for (const formant of formants) {
      const filter = context.createBiquadFilter();
      const gain = context.createGain();
      filter.type = "bandpass";
      filter.frequency.value = formant.frequency;
      filter.Q.value = formant.frequency / formant.bandwidth;
      applyEnvelope(gain.gain, voiceStart, end, formant.gain, 0.012, 0.055);
      voice.connect(filter);
      filter.connect(gain);
      gain.connect(bus);
      nodes.push(filter, gain);
    }

    voice.start(voiceStart);
    vibrato.start(voiceStart);
    voice.stop(end + 0.01);
    vibrato.stop(end + 0.01);
    sources.push(voice, vibrato);
    nodes.push(vibratoDepth);
    return end;
  }

  const soundSchedulers = {
    original: scheduleOriginal,
    arcade: scheduleArcade,
    anime: scheduleAnime
  };

  function clearSoundButton(button) {
    if (!button) return;
    button.classList.remove("is-playing");
    button.removeAttribute("aria-busy");
  }

  function disconnectNodes(nodes) {
    for (const node of nodes) {
      try {
        node.disconnect();
      } catch {
        // A node may already be disconnected after a completed preview.
      }
    }
  }

  function stopActivePreview(announce = false) {
    const preview = activePreview;
    if (!preview || !audioContext) return;
    window.clearTimeout(preview.timer);
    const now = audioContext.currentTime;
    preview.master.gain.cancelScheduledValues(now);
    preview.master.gain.setTargetAtTime(0.0001, now, 0.006);
    for (const source of preview.sources) {
      try {
        source.stop(now + 0.025);
      } catch {
        // Sources can already have reached their scheduled stop time.
      }
    }
    clearSoundButton(preview.button);
    window.setTimeout(() => disconnectNodes(preview.nodes), 45);
    activePreview = null;
    if (announce) setSoundStatus("soundReady");
  }

  function finishPreview(serial, pack) {
    if (!activePreview || activePreview.serial !== serial) return;
    const preview = activePreview;
    clearSoundButton(preview.button);
    disconnectNodes(preview.nodes);
    activePreview = null;
    setSoundStatus("soundFinished", pack);
  }

  async function playPreview(pack, button) {
    const schedule = soundSchedulers[pack];
    if (!schedule) return;
    stopActivePreview(false);
    const serial = ++playbackSerial;
    const context = getAudioContext();
    if (!context) {
      setSoundStatus("soundUnavailable");
      return;
    }

    try {
      await context.resume();
      if (serial !== playbackSerial) return;
      const output = createAudioOutput(context);
      const sources = [];
      const nodes = [...output.nodes];
      const start = context.currentTime + 0.018;
      const end = schedule(context, output.bus, start, sources, nodes);
      button.classList.add("is-playing");
      button.setAttribute("aria-busy", "true");
      setSoundStatus("soundPlaying", pack);
      const timer = window.setTimeout(
        () => finishPreview(serial, pack),
        Math.max(0, (end - context.currentTime) * 1000 + 55)
      );
      activePreview = { serial, pack, button, master: output.master, sources, nodes, timer };
    } catch {
      clearSoundButton(button);
      setSoundStatus("soundError");
    }
  }

  updateVolume();
  volumeInput?.addEventListener("input", updateVolume);

  if (!AudioContextConstructor) {
    for (const button of soundButtons) button.disabled = true;
    setSoundStatus("soundUnavailable");
  } else {
    for (const button of soundButtons) {
      button.addEventListener("click", () => playPreview(button.dataset.soundPreview, button));
    }
  }

  const ngramDemo = document.querySelector("[data-ngram-demo]");
  const ngramReplay = document.querySelector("[data-ngram-replay]");
  const ngramPhases = [...document.querySelectorAll("[data-ngram-phase]")];
  const ngramProgress = [...document.querySelectorAll("[data-ngram-jump]")];
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let traceTimers = [];
  let traceStarted = false;
  let traceFinished = false;

  function clearTraceTimers() {
    for (const timer of traceTimers) window.clearTimeout(timer);
    traceTimers = [];
  }

  function setTracePhase(index) {
    for (const [phaseIndex, phase] of ngramPhases.entries()) {
      phase.hidden = !reducedMotion.matches && phaseIndex !== index;
      phase.classList.toggle("is-active", phaseIndex === index);
      phase.classList.toggle("is-complete", phaseIndex < index);
    }
    for (const [dotIndex, dot] of ngramProgress.entries()) {
      dot.classList.toggle("is-active", dotIndex === index);
      dot.classList.toggle("is-complete", dotIndex < index);
      if (dotIndex === index) dot.setAttribute("aria-current", "step");
      else dot.removeAttribute("aria-current");
    }
  }

  function completeTrace(announce) {
    setTracePhase(3);
    traceFinished = true;
    traceStatusComplete = announce;
    renderTraceStatus();
  }

  function startTrace(announce = false) {
    if (!ngramDemo) return;
    clearTraceTimers();
    traceStarted = true;
    traceFinished = false;
    traceStatusComplete = false;
    renderTraceStatus();
    if (reducedMotion.matches) {
      ngramDemo.classList.add("is-reduced-motion");
      completeTrace(announce);
      return;
    }

    ngramDemo.classList.remove("is-reduced-motion");
    setTracePhase(0);
    const phaseTimes = [1250, 2600, 4450];
    for (let index = 1; index < 4; index += 1) {
      traceTimers.push(window.setTimeout(() => {
        if (index === 3) completeTrace(announce);
        else setTracePhase(index);
      }, phaseTimes[index - 1]));
    }
  }

  function syncMotionPreference() {
    if (!ngramDemo) return;
    clearTraceTimers();
    ngramDemo.classList.toggle("is-reduced-motion", reducedMotion.matches);
    if (reducedMotion.matches) {
      traceStarted = true;
      completeTrace(false);
    } else if (traceStarted) {
      startTrace(false);
    }
  }

  if (typeof reducedMotion.addEventListener === "function") {
    reducedMotion.addEventListener("change", syncMotionPreference);
  } else if (typeof reducedMotion.addListener === "function") {
    reducedMotion.addListener(syncMotionPreference);
  }

  ngramReplay?.addEventListener("click", () => startTrace(true));
  for (const button of ngramProgress) {
    button.addEventListener("click", () => {
      const phase = Number(button.dataset.ngramJump);
      if (!Number.isInteger(phase) || phase < 0 || phase >= ngramPhases.length) return;
      clearTraceTimers();
      traceStarted = true;
      traceFinished = true;
      traceStatusComplete = false;
      renderTraceStatus();
      setTracePhase(phase);
    });
  }

  if (ngramDemo) {
    setTracePhase(0);
    if (reducedMotion.matches) {
      startTrace(false);
    } else if ("IntersectionObserver" in window) {
      const observer = new IntersectionObserver((entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          observer.disconnect();
          if (!traceStarted) startTrace(false);
        }
      }, { threshold: 0.35 });
      observer.observe(ngramDemo);
    } else {
      startTrace(false);
    }
  }

  for (const button of document.querySelectorAll("[data-language]")) {
    button.addEventListener("click", () => setLanguage(button.dataset.language, true));
  }

  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      playbackSerial += 1;
      stopActivePreview(false);
      setSoundStatus("soundReady");
      clearTraceTimers();
    } else if (traceStarted && !traceFinished && !reducedMotion.matches) {
      startTrace(false);
    }
  });

  window.addEventListener("pagehide", () => {
    playbackSerial += 1;
    stopActivePreview(false);
    setSoundStatus("soundReady");
    clearTraceTimers();
    if (audioContext && audioContext.state !== "closed") audioContext.close();
  });

  const requestedLanguage = new URLSearchParams(window.location.search).get("lang");
  setLanguage(requestedLanguage === "uk" ? "uk" : "en", false);
})();
