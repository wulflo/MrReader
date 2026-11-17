# Protocolo de recuperación

1. Lee `docs/CHECKLIST.md` (bloque de estado).
2. Ejecuta `git status` y `git log --oneline -5`.
3. Ejecuta `cargo check` y `cargo test`.
4. Si hay cambios sin commitear que compilan y pasan la verificación del paso en curso: termina el paso, márcalo y haz commit.
5. Si hay cambios que no compilan o están a medias: guarda respaldo con `git stash push -m "recuperacion-<fecha>"` o `git diff > docs/recuperacion.patch`, restaura el último commit estable y rehaz el paso en curso desde cero.
6. Continúa con el siguiente paso sin marcar nada no verificado.
