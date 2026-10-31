# Разработка и устройство расширения

## Разработка и проверка

Нужны Rust stable, Node.js 22 и Python 3. Команды из корня независимого репозитория;
соседние исходники и платформа 1С не требуются.

```
python tools/check.py
python tools/generate-fixture.py target/synthetic.cf
cargo test --locked -p model-server --test rpc
npm --prefix extension test
npm --prefix extension run test:host
python tools/package.py
```

В Linux Host запускается через `xvfb-run -a npm --prefix extension run test:host`.
Runner скачивает официальный VS Code 1.95.3 через test-electron; при необходимости
`VSCODE_EXECUTABLE_PATH` задаёт существующий исполняемый файл VS Code. Всегда
используются отдельные временные workspace/user-data/extensions, пользовательский
профиль не используется. Тест проверяет activation/open/tree/select/properties/
edit/dirty/revision/search/references/composition/save/reopen/XML через настоящий Host.
Обычный check выполняет audit, Rust fmt/check/tests, npm ci/compile и Node process tests.
CI Windows/Linux вызывает те же команды. Chrome не открывается.

Для разработки укажите `cfExtension.serverPath` на `target/debug/model-server[.exe]`
либо положите бинарник в `extension/bin/`. Упаковка собирает release-бинарник и VSIX
в `extension/dist/`. `--target` выбирает совместимую платформу VSIX; кросс-компиляция
не выполняется, несовпадающий target отклоняется. Упаковка ничего не публикует.

## Протокол и синтетика

JSON-RPC 2.0 по stdio с UTF-8 `Content-Length: N\r\n\r\n`, предел 16 MiB.
Stdout — только протокол, stderr — диагностика. Контракт в [protocol.md](protocol.md).
Один сервер владеет Config, ревизией, dirty и файловой транзакцией. Правка проходит
в отдельной модели и заменяет живую только после проверки. Бинарная запись —
собственный уникальный sibling temp, flush/sync и атомарная замена файла.

Генератор `--generate-fixture destination.cf` строит собственный минимальный XML
Configuration с произвольными UUID, именем NavigatorSynthetic и выключенными
мобильными возможностями. XML-оболочку пишет `xml::w::W`, импорт — `xml::imp`,
Catalog Products и Attribute/TabularSection создаются через `edit::add`/`add_child`.
Форматные записи, seed, шаблоны и корпоративные корпуса сюда не копируются.
Генерация использует встроенные ресурсы зависимости; граница описана в
[provenance](provenance.md). BSL, DAP, визуальные редакторы и операции с живой
базой остаются следующими поставками: [roadmap](roadmap.md).

## Лицензия

Код адаптера — MIT. Технический аудит не устанавливает права распространения сторонних материалов.
См. [аудит зависимости](https://github.com/RunetX/cf-unwrapper/blob/v0.1.0/docs/extraction-audit.md)
и [инвентаризацию ресурсов](https://github.com/RunetX/cf-unwrapper/blob/v0.1.0/docs/embedded-assets.md).
