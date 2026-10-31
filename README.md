# Конструктор для VS Code

Самостоятельный клиент метаданных 1С: расширение VS Code и Rust model-server.
Форматы читает библиотека [cf-unwrapper](https://github.com/RunetX/cf-unwrapper/tree/v0.1.0), подключённая как git crate с tag v0.1.0.

Этот первый скаффолд предоставляет одну команду **Конструктор: Открыть конфигурацию**,
запуск сервера и протокол initialize/open/tree. Полный навигатор и редактирование —
следующий коммит: [roadmap](docs/roadmap.md). BSL, DAP и визуальные редакторы сюда не входят.

## Разработка

Нужны Rust stable, Node.js 22 и Python 3. Все команды выполняются из корня этого
независимого репозитория; соседние исходники и платформа 1С не требуются.

```
python tools/check.py
python tools/generate-fixture.py target/synthetic.cf
cargo test --locked -p model-server --test rpc
npm --prefix extension test
npm --prefix extension run test:host
python tools/package.py
```

В Linux Extension Host запускайте через `xvfb-run -a npm --prefix extension run test:host`.
Host runner скачивает официальный VS Code через test-electron, использует временные
workspace/user-data/extensions и удаляет только собственный каталог. Пользовательский профиль не используется.
Обычная проверка не открывает GUI. CI Windows/Linux вызывает те же команды.

Откройте `extension/` в VS Code для разработки; для команды открытия укажите
`cfExtension.serverPath` на `target/debug/model-server[.exe]` либо положите бинарник
в `extension/bin/`. Локальный VSIX содержит release-бинарник, собирается для текущей
платформы в `extension/dist/`. `--target` позволяет указать соответствующий VSIX target;
кросс-компиляция бинарника этим флагом не выполняется. Пользователю VSIX Rust не нужен.
Workspace Trust проверяется до запуска процесса. Команда открытия принимает явный путь
для автоматизации; без него показывает системный диалог.

## Протокол и синтетика

JSON-RPC 2.0 по stdio с UTF-8 `Content-Length: N\r\n\r\n`, максимум 16 MiB.
Stdout содержит только протокол, stderr — диагностику. Имена методов и DTO описаны
в [protocol.md](docs/protocol.md). Ошибка открытия оставляет предыдущую модель.
Сервер читает бинарный контейнер или корневой XML/каталог XML через библиотеку.

Генератор `--generate-fixture destination.cf` строит собственный минимальный XML
Configuration с произвольными UUID, именем NavigatorSynthetic и выключенными
мобильными возможностями. Оболочку XML пишет публичный `xml::w::W`, импорт выполняет
`xml::imp`, Catalog Products и его Attribute/TabularSection добавляют `edit::add` и
`edit::add_child`. Форматные записи, seed, шаблоны и корпоративные примеры сюда не
копируются. Генерация объектов использует встроенные ресурсы зависимости;
[provenance](docs/provenance.md) объясняет эту границу.

## Лицензия

Код адаптера — MIT, см. LICENSE. Технический аудит не устанавливает права распространения сторонних материалов.
См. [аудит зависимости](https://github.com/RunetX/cf-unwrapper/blob/v0.1.0/docs/extraction-audit.md)
и [инвентаризацию встроенных ресурсов](https://github.com/RunetX/cf-unwrapper/blob/v0.1.0/docs/embedded-assets.md).
