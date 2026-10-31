# Происхождение и границы аудита

Новый репозиторий содержит самостоятельно написанные адаптеры, инструменты и
синтетический генератор. Исходники и история constructor/cf-unwrapper, корпоративные
корпуса и платформа не включаются. Форматные решения делегируются git dependency
cf-unwrapper v0.1.0; Cargo.lock фиксирует remote commit.

Внутри этой зависимости есть встроенные seed/templates и другие ресурсы:
[инвентаризация](https://github.com/RunetX/cf-unwrapper/blob/v0.1.0/docs/embedded-assets.md),
[аудит переноса](https://github.com/RunetX/cf-unwrapper/blob/v0.1.0/docs/extraction-audit.md).
Генератор вызывает библиотечные API, которые могут использовать эти встроенные ресурсы.
Технический аудит не устанавливает права распространения сторонних материалов.

`tools/audit.py` проверяет известные path/patch/resource-tree/machine-path patterns,
remote tag в lockfile; это ограниченный технический фильтр. Он не заменяет review
происхождения данных, полного дерева VSIX и прав на сторонние материалы.
