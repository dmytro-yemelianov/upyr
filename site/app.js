(() => {
  "use strict";

  const ukrainian = {
    skip: "Перейти до вмісту",
    navWhy: "Навіщо Upyr",
    navHow: "Як це працює",
    navPrivacy: "Приватність",
    navPlatforms: "Платформи",
    heroEyebrow: "Публічна попередня версія · v0.1.0",
    heroTitle: "Розкладка може помилятися. Ваші слова — ні.",
    heroLead: "Upyr миттєво відновлює англійський та український текст, набраний у неправильній розкладці. Нативна швидкість Rust, локальні рішення, жодного стеження.",
    downloadMac: "Завантажити Upyr для macOS",
    viewSource: "Переглянути код",
    releaseNote: "Універсальний для Apple Silicon та Intel · macOS 11+ · поточне прев’ю має ad-hoc підпис і не нотаризоване Apple",
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
    featureFeedbackCopy: "Необов’язкові прапорці мови біля вказівника та короткі звуки для різних подій підтверджують перемикання, не забираючи фокус.",
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
    packedKey: "16-байтний ключ",
    signedByte: "байт зі знаком",
    privacyKicker: "Приватність — це архітектура",
    privacyTitle: "Ваш набір тексту не має залишати Mac.",
    privacyLead: "Upyr не має системи облікових записів, хмарного інференсу, аналітичного SDK, телеметрії, звітів про збої, реклами чи автоматичних запитів оновлення.",
    securityPolicy: "Безпека та сповіщення",
    privacyMemoryTitle: "Короткочасна пам’ять",
    privacyMemoryCopy: "У пам’яті залишається лише обмежений поточний префікс. Межі та скидання видаляють його; набраний текст не записується в журнали чи на диск.",
    privacyClipboardTitle: "Захищений буфер обміну",
    privacyClipboardCopy: "Коли відновлення ввімкнене, Upyr намагається повернути підтримувані формати та повідомляє про помилки платформи. У macOS тимчасові дані позначені як приховані, а Copy перевіряється нативним лічильником змін.",
    privacyOptTitle: "Автоматизація за бажанням",
    privacyOptCopy: "Автоматичне виправлення, жести модифікаторів, звуки та прапорець мови початково вимкнені. Доступ Accessibility запитується лише за потреби.",
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
    windowsCopy: "Трей-застосунок, мапінг розкладки, підтримка знімка й відновлення буфера, інсталятор, портативний ZIP та CI-перевірки. Місток для скринрідерів у налаштуваннях тимчасово недоступний.",
    statusPreview2: "ПРЕВ’Ю",
    linuxCopy: "XKB-мапінг, GTK-трей, пакунки DEB та tar. Місток для скринрідерів у налаштуваннях тимчасово недоступний; глобальний ввід нативного Wayland очікує дизайну через портали.",
    statusEngine: "ЯДРО",
    wasmCopy: "Незалежне від DOM ядро й згенеровані контракти TypeScript готові. Далі — браузерний адаптер, демо та npm-пакунок.",
    aboutKicker: "Про назву",
    aboutTitle: "Створений перекидатися.",
    aboutCopy: "Назва Upyr походить від українського «упир» — перевертня, того, хто перекидається. Застосунок робить таке саме невелике перетворення з текстом у неправильній розкладці.",
    inspirationCopy: "Це незалежна реалізація на Rust, натхненна Punto Switcher і TolikPylypchuk/KeyboardSwitch, із вужчим фокусом на EN–UK та локальною архітектурою.",
    mitLicense: "Ліцензія MIT",
    downloadKicker: "Публічна попередня версія",
    downloadTitle: "Продовжуйте набирати. Решту перекине Upyr.",
    downloadCopy: "Поточне прев’ю для macOS має ad-hoc підпис і не нотаризоване Apple, тому Gatekeeper може його заблокувати. Також ви можете переглянути й зібрати кожен рядок самостійно.",
    downloadButton: "Завантажити для macOS",
    reportIssue: "Повідомити про проблему",
    footerTagline: "Набирайте те, що мали на увазі.",
    footerSource: "Код",
    footerReleases: "Релізи",
    footerIssues: "Проблеми",
    footerSecurity: "Безпека",
    footerMeta: "Без cookies · Без аналітики · Без зовнішніх ресурсів",
    homeLabel: "Головна сторінка Upyr",
    navLabel: "Основна навігація",
    languageLabel: "Обрати мову",
    githubLabel: "Upyr на GitHub",
    demoLabel: "Приклади перетворення Upyr",
    principlesLabel: "Принципи продукту",
    modelVisualLabel: "Приклади записів підписаних n-грам"
  };

  const englishMetadata = {
    title: "Upyr — type what you meant",
    description: "Upyr is a private, native English–Ukrainian keyboard-layout fixer for macOS, built in Rust and powered by a compact local character n-gram model.",
    socialTitle: "Upyr — type what you meant",
    socialDescription: "Private English ↔ Ukrainian layout correction. Local, fast, open source."
  };

  const ukrainianMetadata = {
    title: "Upyr — набирайте те, що мали на увазі",
    description: "Upyr — приватний нативний перемикач англійської та української розкладок для macOS, створений на Rust із компактною локальною моделлю символьних n-грам.",
    socialTitle: "Upyr — набирайте те, що мали на увазі",
    socialDescription: "Приватне виправлення розкладки EN ↔ УК. Локально, швидко, з відкритим кодом."
  };

  const englishText = new Map();
  const textNodes = [...document.querySelectorAll("[data-i18n]")];
  const ariaNodes = [...document.querySelectorAll("[data-i18n-aria]")];

  for (const node of textNodes) {
    englishText.set(node, node.textContent.trim());
  }

  const englishAria = new Map();
  for (const node of ariaNodes) {
    englishAria.set(node, node.getAttribute("aria-label") || "");
  }

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
    document.documentElement.lang = isUkrainian ? "uk" : "en";

    for (const node of textNodes) {
      const key = node.dataset.i18n;
      node.textContent = isUkrainian ? ukrainian[key] : englishText.get(node);
    }

    for (const node of ariaNodes) {
      const key = node.dataset.i18nAria;
      node.setAttribute("aria-label", isUkrainian ? ukrainian[key] : englishAria.get(node));
    }

    for (const button of document.querySelectorAll("[data-language]")) {
      const selected = button.dataset.language === language;
      button.classList.toggle("is-active", selected);
      button.setAttribute("aria-pressed", String(selected));
    }

    setMetadata(isUkrainian ? ukrainianMetadata : englishMetadata);

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

  for (const button of document.querySelectorAll("[data-language]")) {
    button.addEventListener("click", () => setLanguage(button.dataset.language, true));
  }

  const requestedLanguage = new URLSearchParams(window.location.search).get("lang");
  setLanguage(requestedLanguage === "uk" ? "uk" : "en", false);
})();
