//! UI-only translations. Never apply these to editable values or identifiers.
use crate::config::UiLanguage;
use std::cell::Cell;

thread_local! { static LANGUAGE: Cell<UiLanguage> = const { Cell::new(UiLanguage::Ukrainian) }; }

pub(super) fn set_language(language: UiLanguage) {
    LANGUAGE.set(language);
}
pub(super) fn language() -> UiLanguage {
    LANGUAGE.get()
}
pub(super) fn tr(text: &str) -> &str {
    translate(language(), text)
}

pub(super) fn translate(language: UiLanguage, text: &str) -> &str {
    if language == UiLanguage::English {
        return text;
    }
    match text {
        "Interface language" => "Мова інтерфейсу",
        "Save and reopen settings to apply the language." => {
            "Збережіть і відкрийте налаштування знову, щоб змінити мову."
        }
        "General" => "Основні",
        "Correction" => "Виправлення",
        "Exceptions" => "Винятки",
        "Shortcuts" => "Клавіші",
        "Feedback" => "Вигляд і звуки",
        "Advanced" => "Додатково",
        "About" => "Про програму",
        "Core conversion, clipboard, and startup behavior." => {
            "Перетворення тексту, буфер обміну та запуск програми."
        }
        "Conversion direction" => "Напрямок перетворення",
        "Switch the OS layout to match converted text" => {
            "Перемикати розкладку відповідно до виправленого тексту"
        }
        "Restore clipboard contents after conversion" => {
            "Відновлювати вміст буфера обміну після перетворення"
        }
        "Tap Globe to switch keyboard layouts" => "Перемикати розкладку натисканням Globe",
        "Tap Globe (🌐) to switch keyboard layouts instantly" => {
            "Миттєво перемикати розкладку клавішею Globe (🌐)"
        }
        "Keeps fn shortcuts. Restores macOS handling when paused or quit." => {
            "Зберігає комбінації fn. Після паузи чи виходу повертає керування macOS."
        }
        "Launch Upyr at login" => "Запускати Upyr при вході",
        "Repair Entry" => "Виправити запис",
        "Remove Entry" => "Видалити запис",
        "Automatic correction" => "Автоматичне виправлення",
        "Fix the keyboard layout while typing or at a word boundary." => {
            "Виправлення розкладки під час набору або на межі слова."
        }
        "Correct the wrong keyboard layout automatically" => {
            "Автоматично виправляти помилкову розкладку"
        }
        "Opt-in: a short input prefix is kept only in memory, cleared at safe boundaries, and never logged." => {
            "Короткий початок введеного тексту зберігається лише в пам’яті, вчасно очищується і не потрапляє до журналів."
        }
        "macOS requires Accessibility access. Upyr checks existing access first and offers one restart after permission is granted." => {
            "У macOS потрібен дозвіл у розділі «Доступність». Upyr перевіряє його та пропонує перезапуск після надання дозволу."
        }
        "During a word: clear prefixes (сщте → cont) switch from 4 letters, or your higher minimum. Requires Switch OS layout." => {
            "Усередині слова: однозначні початки (сщте → cont) перемикаються від 4 літер або заданого більшого мінімуму. Увімкніть перемикання розкладки."
        }
        "At a word boundary: short words can wait for context (nj vb → то ми; Ш have → I have)." => {
            "На межі слова: короткі слова можуть чекати на контекст (nj vb → то ми; Ш have → I have)."
        }
        "Exceptions and Keep rules are respected. Processing stays on this device." => {
            "Враховуються винятки та правила Keep. Обробка відбувається на цьому пристрої."
        }
        "Sensitivity" => "Чутливість",
        "Minimum word length" => "Мінімальна довжина слова",
        "Pause after Space" => "Пауза після пробілу",
        "This delay applies after Space; a clear prefix can switch immediately during typing." => {
            "Затримка діє після пробілу; однозначний початок слова може перемкнути розкладку одразу під час набору."
        }
        "Never correct" => "Ніколи не виправляти",
        "Exact words that automatic correction must always leave alone." => {
            "Слова, які автоматичне виправлення завжди має залишати без змін."
        }
        "Word" => "Слово",
        "Remove" => "Видалити",
        "No exceptions yet." => "Винятків поки немає.",
        "words separated by spaces or commas" => "слова через пробіл або кому",
        "Add" => "Додати",
        "Spaces and commas separate words, so \"two words\" adds two exceptions. Case-insensitive duplicates and blank entries are ignored." => {
            "Пробіли й коми розділяють слова: «два слова» додасть два винятки. Порожні записи та повтори незалежно від регістру пропускаються."
        }
        "Click a recorder, then press the combination you want." => {
            "Натисніть поле запису, а потім потрібну комбінацію клавіш."
        }
        "Convert selection" => "Перетворити виділене",
        "Fix previous word" => "Виправити попереднє слово",
        "Undo auto-correction" => "Скасувати автовиправлення",
        "Undo is available for 30 seconds, until further input or navigation." => {
            "Скасування доступне протягом 30 секунд, до наступного вводу або переміщення курсора."
        }
        "Modifier gesture" => "Жест модифікатора",
        "An optional double-tap trigger without a regular key." => {
            "Дія за подвійним натисканням модифікатора без звичайної клавіші."
        }
        "Gesture" => "Жест",
        "Action" => "Дія",
        "Double-tap timeout" => "Інтервал натискань",
        "Press shortcut…  (Esc to cancel)" => "Натисніть комбінацію…  (Esc — скасувати)",
        "Reset" => "Скинути",
        "The shortcut uses physical keys, so it keeps working in either keyboard layout." => {
            "Комбінація використовує фізичні клавіші та працює в обох розкладках."
        }
        "Visual feedback" => "Візуальні індикатори",
        "Optional confirmation after Upyr changes the active input source." => {
            "Візуальне підтвердження після зміни розкладки в Upyr."
        }
        "Show a temporary language flag next to the pointer" => {
            "Показувати прапорець мови біля вказівника"
        }
        "Visible for" => "Тривалість",
        "Shows" => "Вміст",
        "Sound feedback" => "Звукові сигнали",
        "Choose which Upyr actions play a subtle confirmation sound." => {
            "Виберіть дії Upyr, які супроводжуватимуться звуком."
        }
        "Enable sound feedback" => "Увімкнути звукові сигнали",
        "Volume" => "Гучність",
        "Headphone limit" => "Ліміт для навушників",
        "Quiet limit for headphones and unrecognized outputs. Does not change system volume." => {
            "Обмежує гучність для навушників і невідомих пристроїв. Не змінює системну гучність."
        }
        "Sound pack" => "Набір звуків",
        "Keyboard sounds (control-key reactions in Anime pack)" => {
            "Звуки клавіатури (реакції на службові клавіші в наборі Anime)"
        }
        "Preview click" => "Звук натискання",
        "Preview control" => "Звук службової клавіші",
        "Keyboard sounds are opt-in and use only physical key categories; no typed text is retained." => {
            "Звуки клавіатури враховують лише тип фізичної клавіші. Введений текст не зберігається."
        }
        "Preview" => "Прослухати",
        "Preview buttons work even while sound feedback or an individual event is disabled." => {
            "Прослуховування працює навіть коли звуки чи окрему подію вимкнено."
        }
        "Advanced timing" => "Додаткові затримки",
        "Increase these only for applications with slow clipboard handling." => {
            "Збільшуйте лише для програм із повільною обробкою буфера обміну."
        }
        "Copy delay" => "Затримка копіювання",
        "Paste delay" => "Затримка вставлення",
        "Clipboard restore delay" => "Затримка відновлення буфера",
        "English ↔ Ukrainian keyboard layout correction." => {
            "Виправлення розкладки: англійська ↔ українська."
        }
        "English ↔ Ukrainian keyboard layout correction" => {
            "Виправлення розкладки: англійська ↔ українська"
        }
        "Private by construction" => "Приватність за задумом",
        "How it works" => "Як це працює",
        "Open source" => "Відкритий код",
        "Local-only by design: no accounts, analytics, telemetry, ads, or text uploads. Typed text and clipboard contents stay on this device." => {
            "Без облікових записів, аналітики, телеметрії, реклами та надсилання тексту. Введений текст і вміст буфера залишаються на цьому пристрої."
        }
        "Upyr is written in Rust. It maps physical English and Ukrainian keys, then uses a bundled compact character n-gram model to score language candidates locally; no cloud inference is involved." => {
            "Upyr написаний на Rust. Він зіставляє фізичні клавіші англійської та української розкладок і оцінює варіанти локально за компактною моделлю символьних n-грам. Хмарні обчислення не використовуються."
        }
        "MIT licensed. Security reports and implementation details are published with the source." => {
            "Ліцензія MIT. Звіти про безпеку й подробиці реалізації опубліковані разом із кодом."
        }
        "Project website" => "Сайт проєкту",
        "Source repository" => "Репозиторій коду",
        "Report a security issue" => "Повідомити про вразливість",
        "Search results" => "Результати пошуку",
        "Choose a setting to open its tab." => "Виберіть параметр, щоб відкрити його вкладку.",
        "No settings match this search." => "За цим запитом нічого не знайдено.",
        "Upyr Settings" => "Налаштування Upyr",
        "Search settings…" => "Пошук налаштувань…",
        "Save settings" => "Зберегти налаштування",
        "Reset all" => "Скинути все",
        "Reset All" => "Скинути все",
        "Close" => "Закрити",
        "Conservative" => "Обережна",
        "Balanced" => "Збалансована",
        "Aggressive" => "Агресивна",
        "Disabled" => "Вимкнено",
        "Double Control" => "Подвійний Control",
        "Double Shift" => "Подвійний Shift",
        "Double Control + Shift" => "Подвійний Control + Shift",
        "Smart (detect script)" => "Автовизначення",
        "English → Ukrainian" => "Англійська → українська",
        "Ukrainian → English" => "Українська → англійська",
        "Upyr · Pixel Settings" => "Upyr · Піксельні налаштування",
        "CORRECTION" => "ВИПРАВЛЕННЯ",
        "EXCEPTIONS" => "ВИНЯТКИ",
        "Correct the wrong layout automatically" => "Автоматично виправляти помилкову розкладку",
        "Switch OS layout to match corrected text" => {
            "Перемикати розкладку відповідно до виправленого тексту"
        }
        "Minimum word length (2–32)" => "Мінімальна довжина слова (2–32)",
        "Pause after Space (10–250 ms)" => "Пауза після пробілу (10–250 мс)",
        "Clear prefixes can switch immediately while typing." => {
            "Однозначні початки слів можуть перемикати розкладку одразу під час набору."
        }
        "Allow correction in terminals" => "Дозволити виправлення в терміналах",
        "Correct freshly typed /commands in Superset" => {
            "Виправляти щойно введені /команди в Superset"
        }
        "Words Upyr should leave alone. Spaces and commas separate words." => {
            "Слова, які Upyr має залишати без змін. Розділяйте пробілами або комами."
        }
        "Add words" => "Додати слова",
        "UPYR / SETTINGS" => "UPYR / НАЛАШТУВАННЯ",
        "[UNSAVED]" => "[НЕ ЗБЕРЕЖЕНО]",
        "[SAVED]" => "[ЗБЕРЕЖЕНО]",
        "All settings" => "Усі налаштування",
        "Native interface…" => "Системний інтерфейс…",
        "Search settings" => "Пошук налаштувань",
        "Save" => "Зберегти",
        "Revert" => "Відкотити",
        "Reload" => "Перечитати",
        "Defaults" => "Типові значення",
        "Defaults staged. Choose Save to apply them." => {
            "Типові значення підготовлено. Натисніть «Зберегти», щоб застосувати."
        }
        "Discard unsaved changes?" => "Відкинути незбережені зміни?",
        "Your edits have not been saved." => "Ваші зміни ще не збережено.",
        "Keep editing" => "Продовжити редагування",
        "Discard changes" => "Відкинути зміни",
        "Layout flags" => "Прапорці розкладок",
        "Preview layout" => "Розкладка для перегляду",
        "No keyboard layouts" => "Немає розкладок",
        "Refresh layouts" => "Оновити розкладки",
        "Automatic flag" => "Автоматичний прапорець",
        "Preview indicator" => "Показати індикатор",
        "Indicator preview shown." => "Індикатор показано.",
        "Indicator preview unavailable." => "Перегляд індикатора недоступний.",
        "Use Automatic Flag" => "Автоматичний прапорець",
        "All Country Flags…" => "Усі прапорці країн…",
        "Search country name or code" => "Пошук за назвою або кодом країни",
        "All Country Flags" => "Усі прапорці країн",
        "All country flags" => "Усі прапорці країн",
        "Select a keyboard layout, choose a flag, then Save Flags." => {
            "Виберіть розкладку й прапорець, потім натисніть «Зберегти прапорці»."
        }
        "Keyboard layout" => "Розкладка клавіатури",
        "No flags match your search." => "За цим запитом прапорців не знайдено.",
        "Save Flags" => "Зберегти прапорці",
        "Layout name" => "Назва розкладки",
        "Add a keyboard input source in macOS Settings to assign a flag." => {
            "Додайте розкладку в налаштуваннях macOS, щоб призначити прапорець."
        }
        "Preview only. Choose Save Flags to apply your changes." => {
            "Це попередній перегляд. Натисніть «Зберегти прапорці», щоб застосувати зміни."
        }
        "Flag choices are saved." => "Вибрані прапорці збережено.",
        "Saved. Upyr will reload these settings." => "Збережено. Upyr застосує ці налаштування.",
        "Saved. The running background app will reload these settings." => {
            "Збережено. Фонова програма застосує ці налаштування."
        }
        "Saved. The running Upyr process will reload these settings." => {
            "Збережено. Запущений Upyr застосує ці налаштування."
        }
        "Reloaded from disk." => "Налаштування перечитано з диска.",
        "Defaults restored in the form. Choose Save to apply them." => {
            "У формі відновлено типові значення. Натисніть «Зберегти», щоб застосувати."
        }
        "That key cannot be used in a global shortcut." => {
            "Цю клавішу не можна використати в глобальній комбінації."
        }
        "Include Command, Control, Option/Alt, or Shift so normal typing is never captured." => {
            "Додайте Command, Control, Option/Alt або Shift, щоб не перехоплювати звичайний набір тексту."
        }
        "Launch-at-login entry repaired." => "Запис автозапуску виправлено.",
        "Launch-at-login entry removed." => "Запис автозапуску видалено.",
        "Previewed the language flag." => "Прапорець мови показано.",
        "Flag preview updated. Choose Save Flags to apply your choices." => {
            "Перегляд оновлено. Натисніть «Зберегти прапорці», щоб застосувати вибір."
        }
        "Unsaved flag changes reverted." => "Незбережені зміни прапорців відкинуто.",
        "Flags saved." => "Прапорці збережено.",
        "Flags saved. The running Upyr process will reload them." => {
            "Прапорці збережено. Запущений Upyr застосує їх."
        }
        "Opened in your default browser." => "Відкрито у типовому браузері.",
        "Layout preview" => "Перегляд розкладки",
        "A shortcut is incomplete. Record a key together with a modifier." => {
            "Неповна комбінація. Натисніть клавішу разом із модифікатором."
        }
        "Actions must use different shortcuts." => "Для різних дій потрібні різні комбінації.",
        "Shortcuts are valid." => "Комбінації коректні.",
        "That key is not supported." => "Ця клавіша не підтримується.",
        "Unsupported key — try another" => "Непідтримувана клавіша — спробуйте іншу",
        "Include Command, Control, Option, or Shift." => {
            "Додайте Command, Control, Option або Shift."
        }
        "Add a modifier key" => "Додайте клавішу-модифікатор",
        "Pause after Space (ms)" => "Пауза після пробілу (мс)",
        "Opt in for Terminal, iTerm2, Ghostty, kitty and Warp. Upyr holds the next keys while replacing the tracked text; it never sends Enter." => {
            "Для Terminal, iTerm2, Ghostty, kitty та Warp. Upyr притримує наступні клавіші на час заміни тексту та ніколи не надсилає Enter."
        }
        "Click a recorder, then press a physical key combination. Upyr actions are paused here." => {
            "Натисніть поле запису, а потім комбінацію фізичних клавіш. Дії Upyr тут призупинено."
        }
        "Restore the last correction before more typing, clicks or cursor movement." => {
            "Відновити останнє виправлення до наступного набору, клацання чи руху курсора."
        }
        "Gesture action" => "Дія жесту",
        "Double-tap timeout (ms)" => "Інтервал натискання (мс)",
        "Optional visual and sound confirmation for Upyr actions." => {
            "Візуальне та звукове підтвердження дій Upyr."
        }
        "Show the layout indicator" => "Показувати індикатор розкладки",
        "Preview Flag" => "Показати прапорець",
        "Indicator duration (ms)" => "Тривалість індикатора (мс)",
        "GlobeSwitch flags" => "Прапорці GlobeSwitch",
        "Enable sounds" => "Увімкнути звуки",
        "Pack" => "Набір",
        "Anime Reactions synthesizes nonverbal control-key sounds locally; its six event cues are pre-rendered clips bundled with Upyr." => {
            "Anime Reactions локально синтезує звуки службових клавіш. Шість звуків подій постачаються разом з Upyr."
        }
        "Sound volume, 0 to 100 percent" => "Гучність звуку, від 0 до 100 відсотків",
        "Keyboard sounds (Anime adds control-key reactions)" => {
            "Звуки клавіатури (Anime додає реакції на службові клавіші)"
        }
        "Opt-in physical-key categories only. Typed characters are never stored." => {
            "Враховується лише тип фізичної клавіші. Введені символи не зберігаються."
        }
        "Click" => "Натискання",
        "Control" => "Службова клавіша",
        "Quiet limit for Upyr sounds on headphones and unidentified outputs. Does not change macOS volume." => {
            "Ліміт звуків Upyr для навушників і невідомих пристроїв. Не змінює гучність macOS."
        }
        "Copy delay (ms)" => "Затримка копіювання (мс)",
        "Paste delay (ms)" => "Затримка вставлення (мс)",
        "Clipboard restore delay (ms)" => "Відновлення буфера (мс)",
        "MIT licensed. Source, security policy, and release history are public." => {
            "Ліцензія MIT. Код, політика безпеки та історія випусків відкриті."
        }
        "Project Website" => "Сайт проєкту",
        "Source Repository" => "Репозиторій коду",
        "Report Security Issue" => "Повідомити про вразливість",
        "Correct words in terminals" => "Виправляти слова в терміналах",
        "Slash commands in Superset" => "Команди з / у Superset",
        "Globe key switching" => "Перемикання клавішею Globe",
        "Switch OS layout" => "Перемикати розкладку",
        "Restore clipboard" => "Відновлювати буфер обміну",
        "Launch at login" => "Запуск при вході",
        "During a word" => "Усередині слова",
        "Short-word context" => "Контекст коротких слів",
        "Correction delay" => "Затримка виправлення",
        "Never correct list" => "Список винятків",
        "Convert selection shortcut" => "Клавіші перетворення виділеного",
        "Fix previous word shortcut" => "Клавіші виправлення попереднього слова",
        "Gesture timeout" => "Інтервал жесту",
        "Language indicator" => "Індикатор мови",
        "Indicator duration" => "Тривалість індикатора",
        "Indicator style" => "Стиль індикатора",
        "Sound volume" => "Гучність звуку",
        "Keyboard sounds" => "Звуки клавіатури",
        "Event sounds" => "Звуки подій",
        "Version and license" => "Версія та ліцензія",
        "Privacy and implementation" => "Приватність і реалізація",
        "ms" => "мс",
        "Letters" => "Літери",
        "Flag" => "Прапорець",
        "Letters + flag" => "Літери та прапорець",
        "Manual conversion" => "Ручне перетворення",
        "Layout switch" => "Зміна розкладки",
        "Pause" => "Пауза",
        "Resume" => "Відновлення роботи",
        "Error" => "Помилка",
        "Soft local clicks with Upyr's original event cues." => {
            "М’які натискання та оригінальні звуки подій Upyr."
        }
        "Bright pfxr-style clicks and action sounds." => {
            "Виразні натискання та звуки дій у стилі pfxr."
        }
        "Clicks for text keys, synthesized vocal reactions for control keys, and expressive cues for Upyr's own events." => {
            "Натискання текстових клавіш, синтезовані голосові реакції службових клавіш і виразні звуки подій Upyr."
        }
        "Letters and flag" => "Літери та прапорець",
        "typing click" => "натискання текстової клавіші",
        "control-key reaction" => "реакція службової клавіші",
        "Settings saved. " => "Налаштування збережено. ",
        "Launch at login points to another Upyr installation. Choose Repair Entry to update it, or Remove Entry to disable it." => {
            "Автозапуск указує на іншу копію Upyr. Виправте або видаліть запис відповідною кнопкою."
        }
        "The launch-at-login entry needs attention and Upyr will not overwrite it. Choose Remove Entry explicitly before enabling it again." => {
            "Запис автозапуску потребує уваги. Видаліть його відповідною кнопкою перед повторним увімкненням."
        }
        "Minimum word length must be an integer from 2 to 32" => {
            "Мінімальна довжина слова має бути цілим числом від 2 до 32"
        }
        "Pause after Space must be an integer from 10 to 250 ms" => {
            "Пауза після пробілу має бути цілим числом від 10 до 250 мс"
        }
        "Exception row does not exist" => "Такого рядка винятків немає",
        "hotkey must not be empty" => "Комбінація перетворення виділеного не може бути порожньою",
        "last_word_hotkey must not be empty" => {
            "Комбінація виправлення слова не може бути порожньою"
        }
        "hotkey and last_word_hotkey must be different" => {
            "Для виділеного тексту й попереднього слова потрібні різні комбінації"
        }
        "hotkey and last_word_hotkey resolve to the same shortcut" => {
            "Комбінації для виділеного тексту й попереднього слова збігаються"
        }
        "undo_hotkey must differ from the conversion shortcuts" => {
            "Комбінація скасування має відрізнятися від комбінацій перетворення"
        }
        "transliterate_hotkey and case_hotkey must differ from every other shortcut" => {
            "Комбінації транслітерації та зміни регістру мають бути унікальними"
        }
        "copy_delay_ms must be between 10 and 2000" => {
            "Затримка копіювання має бути від 10 до 2000 мс"
        }
        "paste_delay_ms must be at most 2000" => "Затримка вставлення не може перевищувати 2000 мс",
        "restore_delay_ms must be at most 5000" => {
            "Затримка відновлення буфера не може перевищувати 5000 мс"
        }
        "layout_indicator_duration_ms must be between 250 and 3000" => {
            "Тривалість індикатора має бути від 250 до 3000 мс"
        }
        "sounds.volume_percent must be between 0 and 100" => "Гучність має бути від 0 до 100%",
        "sounds.headphone_volume_percent must be between 0 and 30" => {
            "Ліміт для навушників має бути від 0 до 30%"
        }
        "modifier_gesture_timeout_ms must be between 150 and 2000" => {
            "Інтервал подвійного натискання має бути від 150 до 2000 мс"
        }
        "auto_correct_min_word_length must be between 2 and 32" => {
            "Мінімальна довжина слова має бути від 2 до 32 літер"
        }
        "auto_correct_delay_ms must be between 10 and 250" => {
            "Затримка виправлення має бути від 10 до 250 мс"
        }
        "auto_correct_exceptions must contain non-empty single words" => {
            "Винятки мають містити окремі непорожні слова"
        }
        "Use on or off" => "Використайте on або off",
        "Use a row number starting at 1" => "Укажіть номер рядка, починаючи з 1",
        "Rows start at 1" => "Нумерація рядків починається з 1",
        "Choose conservative, balanced or aggressive" => {
            "Виберіть conservative, balanced або aggressive"
        }
        "Use edit ROW WORDS" => "Використайте edit НОМЕР СЛОВА",
        "Choose correction or exceptions" => "Виберіть correction або exceptions",
        "Unknown command. Type help for available commands." => {
            "Невідома команда. Введіть help, щоб переглянути доступні команди."
        }
        "Input ended with unsaved changes; nothing was saved" => {
            "Ввід завершено з незбереженими змінами; нічого не збережено"
        }
        "Unsaved changes. Use save, revert or reload!." => {
            "Є незбережені зміни. Використайте save, revert або reload!."
        }
        "Unsaved changes. Use save, revert or quit!." => {
            "Є незбережені зміни. Використайте save, revert або quit!."
        }
        "Closed without further changes." => "Закрито без подальших змін.",
        "Local editor · Full settings: upyr settings" => {
            "Локальний редактор · Усі налаштування: upyr settings"
        }
        "UNSAVED" => "НЕ ЗБЕРЕЖЕНО",
        "SAVED" => "ЗБЕРЕЖЕНО",
        "speakers" => "динаміки",
        "headphones" => "навушники",
        "unrecognized output (quiet limit)" => "невідомий пристрій (тихий режим)",
        "Saved. Reopen settings to apply the language." => {
            "Збережено. Відкрийте налаштування знову, щоб змінити мову."
        }
        _ => text,
    }
}

/// Backend errors remain intact when a diagnostic is not in the UI catalog.
/// Translate only the known headline; never search/replace inside user values.
pub(super) fn diagnostic(text: &str) -> String {
    if let Some((head, detail)) = text.split_once(": ") {
        format!("{}: {}", tr(head), detail)
    } else {
        tr(text).to_owned()
    }
}

macro_rules! localized_format {
    ($english:literal, $ukrainian:literal $(, $arg:expr)* $(,)?) => {
        if $crate::settings::i18n::language() == $crate::config::UiLanguage::Ukrainian {
            format!($ukrainian $(, $arg)*)
        } else { format!($english $(, $arg)*) }
    };
}
pub(super) use localized_format;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_covers_search_and_keeps_unknown_diagnostics_intact() {
        for language in UiLanguage::ALL {
            set_language(language);
            for item in crate::settings::navigation::SEARCH_PARAMETERS {
                let ukrainian = translate(UiLanguage::Ukrainian, item.label);
                assert_ne!(ukrainian, item.label, "missing {}", item.label);
                assert!(item.matches(ukrainian));
                assert!(item.matches(item.label));
            }
        }
        set_language(UiLanguage::Ukrainian);
        assert_eq!(
            diagnostic(
                "Minimum word length must be an integer from 2 to 32: invalid digit found in string"
            ),
            "Мінімальна довжина слова має бути цілим числом від 2 до 32: invalid digit found in string"
        );
        let unknown = "OS error: /Users/example/General/Save";
        assert_eq!(diagnostic(unknown), unknown);
    }
}
