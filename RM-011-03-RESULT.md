# RM-011 Wave 3 — результат

## Реализация

Implementation commit: `fadd5b97daf1037b9a58ad43d60e7a811e3770f2`

Изменённые файлы:

- `src/app/mod.rs`
- `src/app/actions/poll.rs`
- `src/app/messages.rs`
- `src/app/ui/account.rs`
- `src/i18n.rs`
- `src/session.rs`

Выполнены C1–C3 из `rm-011-client-ux-fix-plan.md`:

- Независимые `pairing` и `account_*` UI-поля заменены единым `AccountUiState` с
  `Disconnected`, `Starting`, `Waiting`, `ConnectedFirstTime`, `Connected` и `Error`.
  Результаты фоновых операций заменяют state целиком.
- При открытии account window запускается фоновая проверка secure credentials и refresh/profile;
  до результата показан progress. `Connect this PC` доступен только в `Disconnected`, а manual
  refresh — только в `Connected`.
- Подтверждённый `401` очищает local credentials и переводит UI в auth-required error. Сеть и
  `5xx` сохраняют connected context и показывают recoverable banner.
- Account title и C3-тексты используют существующий `Lang`; RU title — `Аккаунт и устройства`,
  EN — `Account & devices`.
- Default device name теперь raw hostname с fallback `This PC`; presentation helper добавляет
  `RockCast — ` ровно один раз и нормализует legacy-prefixed name.
- `Starting` блокирует повторный старт pairing request и показывает progress.
- Убрано логирование RockServer URL.

## Проверки

Все команды выполнены успешно:

```text
cargo fmt
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

`cargo test`: 92 passed; live integration tests были ожидаемо ignored.

Добавлены/обновлены проверки:

- Connected и loading screen не предлагают connect/QR/expired UI.
- Raw и legacy `RockCast — ...` names отображаются с одним prefix.
- Refresh `401` удаляет credentials; `5xx` сохраняет credentials и возвращает recoverable outcome.

## Ограничения

- Выполнен только Wave 3 C1–C3. C4–C8 (новый waiting QR UX, success/account-centre flows,
  полная локализация и diagnostics) намеренно не реализовывались.
- Реальные secure-store, passkey и staging pairing flows не выполнялись; для них требуется
  отдельное разрешение и disposable account/request.
