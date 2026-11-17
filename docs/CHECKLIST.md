## Estado actual
- Último paso completado: F10.4
- Paso en curso: (ninguno)
- Último commit estable: c6dc4e8 (tag stable-f10)
- Al retomar: ejecutar el protocolo de recuperación y continuar en la revisión visual final de F10

### Fase 0 — Andamiaje
- [x] F0.1 git init + .gitignore
- [x] F0.2 LICENSE GPL-3.0-or-later
- [x] F0.3 docs/CHECKLIST.md + RECUPERACION.md
- [x] F0.4 Cargo.toml + cargo fetch + commit Cargo.lock
- [x] F0.5 cargo deny / licencias -> docs/LICENCIAS.md

### Fase 1 — analysis + store
- [x] F1.1 Limpieza Markdown a texto plano (`cargo test analysis::clean`)
- [x] F1.2 Tokenización y normalización (`cargo test analysis::tokens`)
- [x] F1.3 Stopwords es/en (`cargo test analysis::stopwords`)
- [x] F1.4 Esquema SQLite y migraciones (`cargo test store::schema`)
- [x] F1.5 Conteo incremental (`cargo test store::counts`)
- [x] F1.6 Top N global e incremental (`cargo test store::top`)
- [x] F1.7 Lectura de líneas desde archivo (`cargo test player::lines`)

### Fase 2 — Indexador
- [x] F2.1 Scanner de carpeta (`cargo test indexer::scan`)
- [x] F2.2 Detección de cambios mtime+tamaño
- [x] F2.3 Pool rayon limitado y prioridad
- [x] F2.4 Procesamiento en streaming de un archivo
- [x] F2.5 Canales acotados con backpressure
- [x] F2.6 Watcher notify con debounce
- [x] F2.7 Reindexado incremental (alta/edición/borrado/renombrado)

### Fase 3 — UI mínima
- [x] F3.1 GtkApplication + ventana
- [x] F3.2 Reproductor splash (línea a línea, fade)
- [x] F3.3 Temporización por longitud + velocidad
- [x] F3.4 Navegación archivos y bucle
- [x] F3.5 Controles ocultables
- [x] F3.6 Indicador de progreso

### Fase 4 — Palabras destacadas
- [x] F4.1 Render Pango markup con escape
- [x] F4.2 Tamaño proporcional (log/sqrt)
- [x] F4.3 Color por niveles
- [x] F4.4 Vista nube de palabras (W)

### Fase 5 — Preferencias y pulido
- [x] F5.1 Archivo de configuración TOML
- [x] F5.2 Diálogo de preferencias
- [x] F5.3 Diálogo Acerca de
- [x] F5.4 Cierre limpio (cancelar workers, commit)

### Fase 6 — Medición y cierre
- [x] F6.1 Script generador 10k archivos
- [x] F6.2 Medición memoria/tiempo -> README
- [x] F6.3 cargo clippy sin warnings
- [x] F6.4 Sin stubs/todo!()
- [x] F6.5 README + docs/LICENCIAS.md

### Fase 7 — Marquesina y estilo
- [x] F7.1 Corregir marquesina con ScrolledWindow y adjustment
- [x] F7.2 Fuente con estilo y glow (CSS)
- [x] F7.3 Verificación final (`cargo test`, `cargo clippy`)
- [x] F7.5 Evitar crecimiento infinito de ventana/RAM en la marquesina
- [x] F7.6 Recorrido completo de la marquesina, pausa real y re-render acotado

### Fase 8 — Preview web de la marquesina
- [x] F8.1 Preview estático `preview/marquee.html` con la misma tipografía, glow y geometría
- [x] F8.2 Verificación visual en navegador (animación, pausa, recorrido completo)

### Fase 9 — Abrir carpeta y tema
- [x] F9.1 Abrir/cambiar carpeta en caliente (diálogo, validación, reindexado)
- [x] F9.2 Ajustar contraste del glow con el fondo
- [x] F9.3 Soporte de tema oscuro para la app y sus diálogos

### Fase 10 — Rediseño aero rojo y pulido
- [x] F10.1 Filtrar propiedades de bullets Anki (`clave:: valor`) del texto indexado y mostrado
- [x] F10.2 Iconos simbólicos en la barra, selector de velocidad por arrastre y switch de tema
- [x] F10.3 Interfaz material aero roja (oscuro/claro) con contorno de texto y glow exterior
- [x] F10.4 Movimiento de la marquesina estable (sincronizado con el frame clock)
