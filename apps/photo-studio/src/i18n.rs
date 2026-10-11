//! Interface languages. Strings are looked up by their English text, so call sites stay
//! readable; anything without a translation falls back to English.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    #[default]
    English,
    Spanish,
    French,
    German,
    Portuguese,
    Italian,
}

impl Lang {
    pub const ALL: [Lang; 6] =
        [Lang::English, Lang::Spanish, Lang::French, Lang::German, Lang::Portuguese, Lang::Italian];

    /// The language's own name, for the picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::English => "English",
            Lang::Spanish => "Español",
            Lang::French => "Français",
            Lang::German => "Deutsch",
            Lang::Portuguese => "Português",
            Lang::Italian => "Italiano",
        }
    }

    /// From a locale such as "de-DE" or "pt_BR".
    pub fn from_locale(locale: &str) -> Lang {
        match locale.get(..2).unwrap_or("").to_ascii_lowercase().as_str() {
            "es" => Lang::Spanish,
            "fr" => Lang::French,
            "de" => Lang::German,
            "pt" => Lang::Portuguese,
            "it" => Lang::Italian,
            _ => Lang::English,
        }
    }
}

thread_local! {
    // The UI runs on one thread; per-thread state also keeps parallel tests independent.
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::English) };
}

pub fn set(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

pub fn current() -> Lang {
    CURRENT.with(Cell::get)
}

/// Translate `s` into the current language. A trailing "…" is kept, "A › B" paths are
/// translated piece by piece, and "context|Text" keys tell apart words with two meanings
/// (the English shown is the part after the bar).
pub fn tr(s: &str) -> Cow<'_, str> {
    let lang = current();
    if lang == Lang::English {
        return Cow::Borrowed(s.split_once('|').map_or(s, |(_, e)| e));
    }
    if let Some((_, english)) = s.split_once('|') {
        return match TABLE.get(s) {
            Some(t) => Cow::Owned(t[lang as usize - 1].to_string()),
            None => Cow::Borrowed(english),
        };
    }
    if s.contains(" › ") {
        return Cow::Owned(s.split(" › ").map(|p| tr(p).into_owned()).collect::<Vec<_>>().join(" › "));
    }
    let (base, dots) = match s.strip_suffix('…') {
        Some(b) => (b, "…"),
        None => (s, ""),
    };
    match TABLE.get(base) {
        Some(t) => Cow::Owned(format!("{}{dots}", t[lang as usize - 1])),
        None => Cow::Borrowed(s),
    }
}

static TABLE: LazyLock<HashMap<&'static str, [&'static str; 5]>> =
    LazyLock::new(|| ENTRIES.iter().chain(crate::i18n_more::MORE).copied().collect());

/// English → [Spanish, French, German, Portuguese, Italian].
const ENTRIES: &[(&str, [&str; 5])] = &[
    // Menus
    ("File", ["Archivo", "Fichier", "Datei", "Arquivo", "File"]),
    ("Edit", ["Edición", "Édition", "Bearbeiten", "Editar", "Modifica"]),
    ("Image", ["Imagen", "Image", "Bild", "Imagem", "Immagine"]),
    ("Layer", ["Capa", "Calque", "Ebene", "Camada", "Livello"]),
    ("Select", ["Selección", "Sélection", "Auswahl", "Selecionar", "Selezione"]),
    ("Filter", ["Filtro", "Filtre", "Filter", "Filtro", "Filtro"]),
    ("View", ["Vista", "Affichage", "Ansicht", "Visualizar", "Vista"]),
    ("Help", ["Ayuda", "Aide", "Hilfe", "Ajuda", "Aiuto"]),
    ("Language", ["Idioma", "Langue", "Sprache", "Idioma", "Lingua"]),
    (
        "Open Recent",
        ["Abrir recientes", "Ouvrir les fichiers récents", "Letzte Dateien öffnen", "Abrir recentes", "Apri recenti"],
    ),
    ("Automate", ["Automatizar", "Automatisation", "Automatisieren", "Automatizar", "Automatizza"]),
    ("Transform", ["Transformar", "Transformation", "Transformieren", "Transformar", "Trasforma"]),
    ("Adjustments", ["Ajustes", "Réglages", "Korrekturen", "Ajustes", "Regolazioni"]),
    (
        "Image Rotation",
        ["Rotación de imagen", "Rotation de l'image", "Bilddrehung", "Rotação da imagem", "Rotazione immagine"],
    ),
    (
        "New Fill Layer",
        [
            "Nueva capa de relleno",
            "Nouveau calque de remplissage",
            "Neue Füllebene",
            "Nova camada de preenchimento",
            "Nuovo livello di riempimento",
        ],
    ),
    (
        "New Adjustment Layer",
        [
            "Nueva capa de ajuste",
            "Nouveau calque de réglage",
            "Neue Einstellungsebene",
            "Nova camada de ajuste",
            "Nuovo livello di regolazione",
        ],
    ),
    ("Layer Style", ["Estilo de capa", "Style de calque", "Ebenenstil", "Estilo de camada", "Stile livello"]),
    ("Layer Mask", ["Máscara de capa", "Masque de fusion", "Ebenenmaske", "Máscara de camada", "Maschera di livello"]),
    ("Arrange", ["Organizar", "Disposition", "Anordnen", "Organizar", "Disponi"]),
    (
        "Load Selection",
        ["Cargar selección", "Récupérer la sélection", "Auswahl laden", "Carregar seleção", "Carica selezione"],
    ),
    ("Modify", ["Modificar", "Modifier", "Ändern", "Modificar", "Modifica"]),
    // File
    ("New", ["Nuevo", "Nouveau", "Neu", "Novo", "Nuovo"]),
    (
        "New from Clipboard",
        [
            "Nuevo desde el portapapeles",
            "Nouveau à partir du presse-papiers",
            "Neu aus Zwischenablage",
            "Novo da área de transferência",
            "Nuovo dagli appunti",
        ],
    ),
    ("Open", ["Abrir", "Ouvrir", "Öffnen", "Abrir", "Apri"]),
    ("Close", ["Cerrar", "Fermer", "Schließen", "Fechar", "Chiudi"]),
    ("Save", ["Guardar", "Enregistrer", "Speichern", "Salvar", "Salva"]),
    ("Save As", ["Guardar como", "Enregistrer sous", "Speichern unter", "Salvar como", "Salva con nome"]),
    ("Export As", ["Exportar como", "Exporter sous", "Exportieren als", "Exportar como", "Esporta come"]),
    (
        "Quick Export as PNG",
        [
            "Exportación rápida como PNG",
            "Exportation rapide au format PNG",
            "Schnell-Export als PNG",
            "Exportação rápida como PNG",
            "Esportazione rapida come PNG",
        ],
    ),
    (
        "Export Slices",
        ["Exportar sectores", "Exporter les tranches", "Slices exportieren", "Exportar fatias", "Esporta sezioni"],
    ),
    (
        "Clear Notes, Counts and Slices",
        [
            "Borrar notas, recuentos y sectores",
            "Effacer notes, comptages et tranches",
            "Notizen, Zählungen und Slices löschen",
            "Limpar notas, contagens e fatias",
            "Cancella note, conteggi e sezioni",
        ],
    ),
    ("Revert", ["Volver", "Version précédente", "Zurück zur letzten Version", "Reverter", "Ripristina"]),
    ("Quit", ["Salir", "Quitter", "Beenden", "Sair", "Esci"]),
    ("Batch", ["Lote", "Traitement par lots", "Stapelverarbeitung", "Lote", "Elabora in batch"]),
    (
        "Start Recording",
        [
            "Iniciar grabación",
            "Démarrer l'enregistrement",
            "Aufzeichnung beginnen",
            "Iniciar gravação",
            "Avvia registrazione",
        ],
    ),
    (
        "Stop Recording",
        [
            "Detener grabación",
            "Arrêter l'enregistrement",
            "Aufzeichnung beenden",
            "Parar gravação",
            "Interrompi registrazione",
        ],
    ),
    ("Play Action", ["Ejecutar acción", "Exécuter l'action", "Aktion ausführen", "Executar ação", "Esegui azione"]),
    ("Delete Action", ["Eliminar acción", "Supprimer l'action", "Aktion löschen", "Excluir ação", "Elimina azione"]),
    (
        "Duplicate Image",
        ["Duplicar imagen", "Dupliquer l'image", "Bild duplizieren", "Duplicar imagem", "Duplica immagine"],
    ),
    ("Preferences", ["Preferencias", "Préférences", "Voreinstellungen", "Preferências", "Preferenze"]),
    // Edit
    ("Undo", ["Deshacer", "Annuler", "Rückgängig", "Desfazer", "Annulla"]),
    ("Redo", ["Rehacer", "Rétablir", "Wiederholen", "Refazer", "Ripeti"]),
    ("Step Backward", ["Paso atrás", "Aller un pas en arrière", "Schritt zurück", "Voltar um passo", "Passo indietro"]),
    ("Cut", ["Cortar", "Couper", "Ausschneiden", "Recortar", "Taglia"]),
    ("Copy", ["Copiar", "Copier", "Kopieren", "Copiar", "Copia"]),
    (
        "Copy Merged",
        [
            "Copiar combinado",
            "Copier avec fusion",
            "Auf eine Ebene reduziert kopieren",
            "Copiar mesclado",
            "Copia unione",
        ],
    ),
    ("Paste", ["Pegar", "Coller", "Einfügen", "Colar", "Incolla"]),
    (
        "Paste in Place",
        [
            "Pegar en el sitio",
            "Coller sur place",
            "An Originalposition einfügen",
            "Colar no local",
            "Incolla sul posto",
        ],
    ),
    ("Paste Into", ["Pegar dentro", "Coller dedans", "In die Auswahl einfügen", "Colar dentro", "Incolla dentro"]),
    ("Clear", ["Borrar", "Effacer", "Löschen", "Limpar", "Cancella"]),
    ("Fill", ["Rellenar", "Remplir", "Fläche füllen", "Preencher", "Riempi"]),
    ("Stroke", ["Contornear", "Contour", "Kontur füllen", "Traçado", "Traccia"]),
    (
        "Content-Aware Fill",
        [
            "Relleno según el contenido",
            "Remplissage d'après le contenu",
            "Inhaltsbasierte Füllung",
            "Preenchimento sensível ao conteúdo",
            "Riempimento in base al contenuto",
        ],
    ),
    (
        "Free Transform",
        [
            "Transformación libre",
            "Transformation manuelle",
            "Frei transformieren",
            "Transformação livre",
            "Trasformazione libera",
        ],
    ),
    (
        "Define Pattern",
        ["Definir motivo", "Utiliser comme motif", "Muster festlegen", "Definir padrão", "Definisci pattern"],
    ),
    (
        "Swap Colors",
        ["Intercambiar colores", "Permuter les couleurs", "Farben vertauschen", "Trocar cores", "Inverti colori"],
    ),
    (
        "Default Colors",
        ["Colores por defecto", "Couleurs par défaut", "Standardfarben", "Cores padrão", "Colori predefiniti"],
    ),
    (
        "Decrease Brush Size",
        [
            "Reducir tamaño del pincel",
            "Réduire la taille de la forme",
            "Pinselgröße verringern",
            "Diminuir tamanho do pincel",
            "Riduci dimensione pennello",
        ],
    ),
    (
        "Increase Brush Size",
        [
            "Aumentar tamaño del pincel",
            "Augmenter la taille de la forme",
            "Pinselgröße erhöhen",
            "Aumentar tamanho do pincel",
            "Aumenta dimensione pennello",
        ],
    ),
    (
        "Flip Horizontal",
        [
            "Voltear horizontal",
            "Symétrie axe vertical",
            "Horizontal spiegeln",
            "Inverter horizontal",
            "Rifletti orizzontale",
        ],
    ),
    (
        "Flip Vertical",
        ["Voltear vertical", "Symétrie axe horizontal", "Vertikal spiegeln", "Inverter vertical", "Rifletti verticale"],
    ),
    ("Rotate 180°", ["Rotar 180°", "Rotation 180°", "180° drehen", "Girar 180°", "Ruota 180°"]),
    (
        "Rotate 90° Clockwise",
        [
            "Rotar 90° en sentido horario",
            "Rotation 90° horaire",
            "90° im UZS drehen",
            "Girar 90° no sentido horário",
            "Ruota 90° in senso orario",
        ],
    ),
    (
        "Rotate 90° Counter Clockwise",
        [
            "Rotar 90° en sentido antihorario",
            "Rotation 90° antihoraire",
            "90° gegen UZS drehen",
            "Girar 90° no sentido anti-horário",
            "Ruota 90° in senso antiorario",
        ],
    ),
    (
        "90° Clockwise",
        ["90° en sentido horario", "90° horaire", "90° im UZS", "90° no sentido horário", "90° in senso orario"],
    ),
    (
        "90° Counter Clockwise",
        [
            "90° en sentido antihorario",
            "90° antihoraire",
            "90° gegen UZS",
            "90° no sentido anti-horário",
            "90° in senso antiorario",
        ],
    ),
    ("Arbitrary", ["Arbitrario", "Paramétrée", "Per Eingabe", "Arbitrário", "Arbitraria"]),
    (
        "Flip Canvas Horizontal",
        [
            "Voltear lienzo horizontal",
            "Symétrie de la zone de travail axe vertical",
            "Arbeitsfläche horizontal spiegeln",
            "Inverter tela horizontal",
            "Rifletti quadro orizzontale",
        ],
    ),
    (
        "Flip Canvas Vertical",
        [
            "Voltear lienzo vertical",
            "Symétrie de la zone de travail axe horizontal",
            "Arbeitsfläche vertikal spiegeln",
            "Inverter tela vertical",
            "Rifletti quadro verticale",
        ],
    ),
    // Image
    ("Image Size", ["Tamaño de imagen", "Taille de l'image", "Bildgröße", "Tamanho da imagem", "Dimensione immagine"]),
    (
        "Canvas Size",
        ["Tamaño de lienzo", "Taille de la zone de travail", "Arbeitsfläche", "Tamanho da tela", "Dimensione quadro"],
    ),
    ("Crop", ["Recortar", "Recadrer", "Freistellen", "Cortar", "Ritaglia"]),
    (
        "Trim Transparent Pixels",
        [
            "Cortar píxeles transparentes",
            "Rogner les pixels transparents",
            "Transparente Pixel zuschneiden",
            "Aparar pixels transparentes",
            "Rifila pixel trasparenti",
        ],
    ),
    (
        "Straighten with Ruler",
        [
            "Enderezar con la regla",
            "Redresser avec la règle",
            "Mit Linealwerkzeug gerade ausrichten",
            "Endireitar com a régua",
            "Raddrizza con il righello",
        ],
    ),
    ("Auto Tone", ["Tono automático", "Tons automatiques", "Auto-Farbton", "Tom automático", "Tono automatico"]),
    (
        "Auto Contrast",
        [
            "Contraste automático",
            "Contraste automatique",
            "Auto-Kontrast",
            "Contraste automático",
            "Contrasto automatico",
        ],
    ),
    ("Auto Color", ["Color automático", "Couleur automatique", "Auto-Farbe", "Cor automática", "Colore automatico"]),
    (
        "Brightness/Contrast",
        ["Brillo/Contraste", "Luminosité/Contraste", "Helligkeit/Kontrast", "Brilho/Contraste", "Luminosità/Contrasto"],
    ),
    ("Levels", ["Niveles", "Niveaux", "Tonwertkorrektur", "Níveis", "Livelli"]),
    ("Curves", ["Curvas", "Courbes", "Gradationskurven", "Curvas", "Curve"]),
    ("Exposure", ["Exposición", "Exposition", "Belichtung", "Exposição", "Esposizione"]),
    ("Vibrance", ["Intensidad", "Vibrance", "Dynamik", "Vibratilidade", "Vividezza"]),
    (
        "Hue/Saturation",
        ["Tono/Saturación", "Teinte/Saturation", "Farbton/Sättigung", "Matiz/Saturação", "Tonalità/Saturazione"],
    ),
    (
        "Color Balance",
        ["Equilibrio de color", "Balance des couleurs", "Farbbalance", "Equilíbrio de cores", "Bilanciamento colore"],
    ),
    ("Black & White", ["Blanco y negro", "Noir et blanc", "Schwarzweiß", "Preto e branco", "Bianco e nero"]),
    ("Photo Filter", ["Filtro de fotografía", "Filtre photo", "Fotofilter", "Filtro de fotos", "Filtro fotografico"]),
    (
        "Channel Mixer",
        ["Mezclador de canales", "Mélangeur de couches", "Kanalmixer", "Misturador de canais", "Miscelatore canale"],
    ),
    ("Invert", ["Invertir", "Négatif", "Umkehren", "Inverter", "Inverti"]),
    ("Posterize", ["Posterizar", "Isohélie", "Tontrennung", "Posterizar", "Posterizza"]),
    ("Threshold", ["Umbral", "Seuil", "Schwellenwert", "Limiar", "Soglia"]),
    (
        "Gradient Map",
        [
            "Mapa de degradado",
            "Courbe de transfert de dégradé",
            "Verlaufsumsetzung",
            "Mapa de gradiente",
            "Mappa sfumatura",
        ],
    ),
    (
        "Selective Color",
        [
            "Corrección selectiva",
            "Correction sélective",
            "Selektive Farbkorrektur",
            "Cor seletiva",
            "Correzione colore selettiva",
        ],
    ),
    (
        "Shadows/Highlights",
        ["Sombras/Iluminaciones", "Tons foncés/Tons clairs", "Tiefen/Lichter", "Sombras/Realces", "Ombre/Luci"],
    ),
    ("Desaturate", ["Desaturar", "Désaturation", "Sättigung verringern", "Remover saturação", "Togli saturazione"]),
    ("Equalize", ["Ecualizar", "Égaliser", "Tonwertangleichung", "Equalizar", "Equalizza"]),
    ("Reds", ["Rojos", "Rouges", "Rottöne", "Vermelhos", "Rossi"]),
    ("Yellows", ["Amarillos", "Jaunes", "Gelbtöne", "Amarelos", "Gialli"]),
    ("Greens", ["Verdes", "Verts", "Grüntöne", "Verdes", "Verdi"]),
    ("Cyans", ["Cianes", "Cyans", "Cyantöne", "Cianos", "Ciano"]),
    ("Blues", ["Azules", "Bleus", "Blautöne", "Azuis", "Blu"]),
    ("Magentas", ["Magentas", "Magentas", "Magentatöne", "Magentas", "Magenta"]),
    ("Whites", ["Blancos", "Blancs", "Weiß", "Brancos", "Bianchi"]),
    ("Neutrals", ["Neutros", "Neutres", "Neutrale Farben", "Neutros", "Neutri"]),
    ("Blacks", ["Negros", "Noirs", "Schwarz", "Pretos", "Neri"]),
    // Layer
    ("New Layer", ["Nueva capa", "Nouveau calque", "Neue Ebene", "Nova camada", "Nuovo livello"]),
    ("New Group", ["Nuevo grupo", "Nouveau groupe", "Neue Gruppe", "Novo grupo", "Nuovo gruppo"]),
    (
        "Duplicate Layer",
        ["Duplicar capa", "Dupliquer le calque", "Ebene duplizieren", "Duplicar camada", "Duplica livello"],
    ),
    ("Delete Layer", ["Eliminar capa", "Supprimer le calque", "Ebene löschen", "Excluir camada", "Elimina livello"]),
    (
        "Layer via Copy",
        ["Capa vía copiar", "Calque par copier", "Ebene durch Kopie", "Camada via cópia", "Livello tramite copia"],
    ),
    (
        "Layer via Cut",
        [
            "Capa vía cortar",
            "Calque par couper",
            "Ebene durch Ausschneiden",
            "Camada via recorte",
            "Livello tramite taglio",
        ],
    ),
    (
        "Group Layers",
        ["Agrupar capas", "Grouper des calques", "Ebenen gruppieren", "Agrupar camadas", "Raggruppa livelli"],
    ),
    (
        "Ungroup Layers",
        [
            "Desagrupar capas",
            "Dissocier les calques",
            "Ebenengruppierung aufheben",
            "Desagrupar camadas",
            "Separa livelli",
        ],
    ),
    (
        "Merge Down",
        [
            "Combinar hacia abajo",
            "Fusionner avec le calque inférieur",
            "Auf eine Ebene reduzieren",
            "Mesclar para baixo",
            "Unisci sotto",
        ],
    ),
    (
        "Merge Visible",
        [
            "Combinar visibles",
            "Fusionner les calques visibles",
            "Sichtbare auf eine Ebene reduzieren",
            "Mesclar visíveis",
            "Unisci visibili",
        ],
    ),
    (
        "Flatten Image",
        ["Acoplar imagen", "Aplatir l'image", "Auf Hintergrundebene reduzieren", "Achatar imagem", "Unico livello"],
    ),
    (
        "Rasterize Layer",
        ["Rasterizar capa", "Pixelliser le calque", "Ebene rastern", "Rasterizar camada", "Rasterizza livello"],
    ),
    (
        "Convert to Smart Object",
        [
            "Convertir en objeto inteligente",
            "Convertir en objet dynamique",
            "In Smartobjekt konvertieren",
            "Converter em objeto inteligente",
            "Converti in oggetto avanzato",
        ],
    ),
    (
        "Create / Release Clipping Mask",
        [
            "Crear/soltar máscara de recorte",
            "Créer/annuler un masque d'écrêtage",
            "Schnittmaske erstellen/zurückwandeln",
            "Criar/liberar máscara de corte",
            "Crea/rilascia maschera di ritaglio",
        ],
    ),
    (
        "Clear Layer Style",
        [
            "Borrar estilo de capa",
            "Effacer le style de calque",
            "Ebenenstil löschen",
            "Limpar estilo de camada",
            "Cancella stile livello",
        ],
    ),
    ("Reveal All", ["Descubrir todas", "Tout faire apparaître", "Alles einblenden", "Revelar tudo", "Mostra tutto"]),
    ("Hide All", ["Ocultar todas", "Tout masquer", "Alles ausblenden", "Ocultar tudo", "Nascondi tutto"]),
    (
        "Reveal Selection",
        [
            "Descubrir selección",
            "Faire apparaître la sélection",
            "Auswahl einblenden",
            "Revelar seleção",
            "Mostra selezione",
        ],
    ),
    (
        "Disable / Enable",
        ["Desactivar/activar", "Désactiver/activer", "Deaktivieren/aktivieren", "Desativar/ativar", "Disattiva/attiva"],
    ),
    ("Delete", ["Eliminar", "Supprimer", "Löschen", "Excluir", "Elimina"]),
    ("Apply", ["Aplicar", "Appliquer", "Anwenden", "Aplicar", "Applica"]),
    (
        "Vector Mask from Current Path",
        [
            "Máscara vectorial del trazado actual",
            "Masque vectoriel du tracé actif",
            "Vektormaske aus aktuellem Pfad",
            "Máscara vetorial do demarcador atual",
            "Maschera vettoriale dal tracciato corrente",
        ],
    ),
    (
        "Delete Vector Mask",
        [
            "Eliminar máscara vectorial",
            "Supprimer le masque vectoriel",
            "Vektormaske löschen",
            "Excluir máscara vetorial",
            "Elimina maschera vettoriale",
        ],
    ),
    (
        "Bring to Front",
        ["Traer al frente", "Premier plan", "In den Vordergrund", "Trazer para a frente", "Porta in primo piano"],
    ),
    ("Bring Forward", ["Hacia delante", "En avant", "Schrittweise vorwärts", "Avançar", "Porta avanti"]),
    ("Send Backward", ["Hacia atrás", "En arrière", "Schrittweise rückwärts", "Recuar", "Porta indietro"]),
    ("Send to Back", ["Enviar detrás", "Arrière-plan", "In den Hintergrund", "Enviar para trás", "Porta sotto"]),
    (
        "Select Layer Above",
        [
            "Seleccionar capa superior",
            "Sélectionner le calque supérieur",
            "Darüberliegende Ebene auswählen",
            "Selecionar camada acima",
            "Seleziona livello sopra",
        ],
    ),
    (
        "Select Layer Below",
        [
            "Seleccionar capa inferior",
            "Sélectionner le calque inférieur",
            "Darunterliegende Ebene auswählen",
            "Selecionar camada abaixo",
            "Seleziona livello sotto",
        ],
    ),
    (
        "Align Top Edges",
        [
            "Alinear bordes superiores",
            "Aligner les bords supérieurs",
            "Oberkanten ausrichten",
            "Alinhar bordas superiores",
            "Allinea bordi superiori",
        ],
    ),
    (
        "Align Vertical Centers",
        [
            "Alinear centros verticales",
            "Aligner les centres verticalement",
            "Vertikale Mitte ausrichten",
            "Alinhar centros verticais",
            "Allinea centri verticali",
        ],
    ),
    (
        "Align Bottom Edges",
        [
            "Alinear bordes inferiores",
            "Aligner les bords inférieurs",
            "Unterkanten ausrichten",
            "Alinhar bordas inferiores",
            "Allinea bordi inferiori",
        ],
    ),
    (
        "Align Left Edges",
        [
            "Alinear bordes izquierdos",
            "Aligner les bords gauches",
            "Linke Kanten ausrichten",
            "Alinhar bordas esquerdas",
            "Allinea bordi sinistri",
        ],
    ),
    (
        "Align Horizontal Centers",
        [
            "Alinear centros horizontales",
            "Aligner les centres horizontalement",
            "Horizontale Mitte ausrichten",
            "Alinhar centros horizontais",
            "Allinea centri orizzontali",
        ],
    ),
    (
        "Align Right Edges",
        [
            "Alinear bordes derechos",
            "Aligner les bords droits",
            "Rechte Kanten ausrichten",
            "Alinhar bordas direitas",
            "Allinea bordi destri",
        ],
    ),
    (
        "Distribute Horizontal Centers",
        [
            "Distribuir centros horizontales",
            "Répartir les centres horizontalement",
            "Horizontale Mitten verteilen",
            "Distribuir centros horizontais",
            "Distribuisci centri orizzontali",
        ],
    ),
    (
        "Distribute Vertical Centers",
        [
            "Distribuir centros verticales",
            "Répartir les centres verticalement",
            "Vertikale Mitten verteilen",
            "Distribuir centros verticais",
            "Distribuisci centri verticali",
        ],
    ),
    ("Solid Color", ["Color uniforme", "Couleur unie", "Volltonfarbe", "Cor sólida", "Tinta unita"]),
    ("Gradient", ["Degradado", "Dégradé", "Verlauf", "Gradiente", "Sfumatura"]),
    ("Pattern", ["Motivo", "Motif", "Muster", "Padrão", "Pattern"]),
    // Select
    ("All", ["Todo", "Tout sélectionner", "Alles auswählen", "Tudo", "Tutto"]),
    ("Deselect", ["Deseleccionar", "Désélectionner", "Auswahl aufheben", "Desmarcar", "Deseleziona"]),
    ("Reselect", ["Volver a seleccionar", "Resélectionner", "Erneut auswählen", "Selecionar novamente", "Riseleziona"]),
    ("Inverse", ["Invertir", "Intervertir", "Auswahl umkehren", "Inverter", "Inversa"]),
    ("Subject", ["Sujeto", "Sujet", "Motiv", "Assunto", "Soggetto"]),
    ("Color Range", ["Gama de colores", "Plage de couleurs", "Farbbereich", "Intervalo de cores", "Intervallo colori"]),
    (
        "Edit in Quick Mask Mode",
        [
            "Editar en modo máscara rápida",
            "Modifier en mode Masque",
            "Im Maskierungsmodus bearbeiten",
            "Editar no modo máscara rápida",
            "Modifica in modalità maschera veloce",
        ],
    ),
    (
        "Save Selection",
        ["Guardar selección", "Mémoriser la sélection", "Auswahl speichern", "Salvar seleção", "Salva selezione"],
    ),
    (
        "Load Layer Transparency",
        [
            "Cargar transparencia de capa",
            "Récupérer la transparence du calque",
            "Ebenentransparenz laden",
            "Carregar transparência da camada",
            "Carica trasparenza livello",
        ],
    ),
    ("Delete Channel", ["Eliminar canal", "Supprimer la couche", "Kanal löschen", "Excluir canal", "Elimina canale"]),
    (
        "Make Selection from Path",
        [
            "Hacer selección del trazado",
            "Créer une sélection à partir du tracé",
            "Auswahl aus Pfad erstellen",
            "Fazer seleção do demarcador",
            "Crea selezione dal tracciato",
        ],
    ),
    (
        "Fill Path",
        ["Rellenar trazado", "Fond du tracé", "Pfadfläche füllen", "Preencher demarcador", "Riempi tracciato"],
    ),
    (
        "Stroke Path",
        ["Contornear trazado", "Contour du tracé", "Pfadkontur füllen", "Traçar demarcador", "Traccia tracciato"],
    ),
    (
        "Shape Layer from Path",
        [
            "Capa de forma del trazado",
            "Calque de forme à partir du tracé",
            "Formebene aus Pfad",
            "Camada de forma do demarcador",
            "Livello forma dal tracciato",
        ],
    ),
    (
        "Delete Path",
        ["Eliminar trazado", "Supprimer le tracé", "Pfad löschen", "Excluir demarcador", "Elimina tracciato"],
    ),
    // Filter
    (
        "Repeat Last Filter",
        [
            "Repetir último filtro",
            "Répéter le dernier filtre",
            "Letzten Filter wiederholen",
            "Repetir último filtro",
            "Ripeti ultimo filtro",
        ],
    ),
    ("Blur", ["Desenfocar", "Flou", "Weichzeichnungsfilter", "Desfoque", "Sfocatura"]),
    ("Sharpen", ["Enfocar", "Renforcement", "Scharfzeichnen", "Tornar nítido", "Contrasta"]),
    ("Noise", ["Ruido", "Bruit", "Rauschfilter", "Ruído", "Disturbo"]),
    ("Distort", ["Distorsionar", "Déformation", "Verzerrungsfilter", "Distorcer", "Distorsione"]),
    ("Pixelate", ["Pixelizar", "Pixellisation", "Vergröberungsfilter", "Pixelizar", "Effetto pixel"]),
    ("Stylize", ["Estilizar", "Esthétiques", "Stilisierungsfilter", "Estilizar", "Stilizzazione"]),
    ("Render", ["Interpretar", "Rendu", "Renderfilter", "Acabamento", "Rendering"]),
    (
        "Gaussian Blur",
        [
            "Desenfoque gaussiano",
            "Flou gaussien",
            "Gaußscher Weichzeichner",
            "Desfoque gaussiano",
            "Controllo sfocatura",
        ],
    ),
    (
        "Box Blur",
        [
            "Desenfoque de cuadro",
            "Flou de l'objectif",
            "Matter machen (Box)",
            "Desfoque de caixa",
            "Sfocatura riquadro",
        ],
    ),
    (
        "Motion Blur",
        [
            "Desenfoque de movimiento",
            "Flou directionnel",
            "Bewegungsunschärfe",
            "Desfoque de movimento",
            "Sfocatura movimento",
        ],
    ),
    (
        "Radial Blur",
        ["Desenfoque radial", "Flou radial", "Radialer Weichzeichner", "Desfoque radial", "Sfocatura radiale"],
    ),
    (
        "Surface Blur",
        [
            "Desenfoque de superficie",
            "Flou de surface",
            "Matter machen",
            "Desfoque de superfície",
            "Controllo sfocatura superficie",
        ],
    ),
    (
        "Tilt-Shift",
        ["Inclinación-desplazamiento", "Inclinaison-décalage", "Tilt-Shift", "Inclinação-deslocamento", "Tilt-Shift"],
    ),
    (
        "Iris Blur",
        ["Desenfoque de iris", "Flou de diaphragme", "Iris-Weichzeichnung", "Desfoque de íris", "Sfocatura diaframma"],
    ),
    (
        "Field Blur",
        ["Desenfoque de campo", "Flou de champ", "Feld-Weichzeichnung", "Desfoque de campo", "Sfocatura campo"],
    ),
    (
        "Path Blur",
        [
            "Desenfoque de trazado",
            "Flou de tracé",
            "Pfad-Weichzeichnung",
            "Desfoque de demarcador",
            "Sfocatura tracciato",
        ],
    ),
    (
        "Sharpen More",
        ["Enfocar más", "Renforcement plus", "Stärker scharfzeichnen", "Tornar mais nítido", "Contrasta maggiormente"],
    ),
    (
        "Unsharp Mask",
        ["Máscara de enfoque", "Accentuation", "Unscharf maskieren", "Máscara de nitidez", "Maschera di contrasto"],
    ),
    ("Add Noise", ["Añadir ruido", "Ajout de bruit", "Rauschen hinzufügen", "Adicionar ruído", "Aggiungi disturbo"]),
    ("Median", ["Mediana", "Médiane", "Median", "Mediana", "Mediana"]),
    (
        "Reduce Noise",
        ["Reducir ruido", "Réduction du bruit", "Rauschen reduzieren", "Reduzir ruído", "Riduci disturbo"],
    ),
    (
        "Dust & Scratches",
        ["Polvo y rascaduras", "Antipoussière", "Staub und Kratzer", "Poeira e arranhões", "Polvere e grana"],
    ),
    ("Twirl", ["Molinete", "Tourbillon", "Strudel", "Redemoinho", "Mulinello"]),
    ("Wave", ["Onda", "Ondulation", "Wellen", "Onda", "Onda"]),
    ("Ripple", ["Rizo", "Ondes", "Kräuseln", "Ondulação", "Increspatura"]),
    ("Pinch", ["Pellizcar", "Pincement", "Wölben", "Pinçar", "Contrazione"]),
    ("Spherize", ["Esferizar", "Sphérisation", "Wölbung", "Esferizar", "Sferizza"]),
    (
        "Polar Coordinates",
        ["Coordenadas polares", "Coordonnées polaires", "Polarkoordinaten", "Coordenadas polares", "Coordinate polari"],
    ),
    ("Mosaic", ["Mosaico", "Mosaïque", "Mosaikeffekt", "Mosaico", "Mosaico"]),
    ("Crystallize", ["Cristalizar", "Cristallisation", "Kristallisieren", "Cristalizar", "Cristallizza"]),
    (
        "Color Halftone",
        ["Semitono de color", "Demi-teintes couleur", "Farbraster", "Meio-tom colorido", "Mezzetinte colore"],
    ),
    ("Emboss", ["Relieve", "Estampage", "Relief", "Relevo", "Rilievo"]),
    ("Find Edges", ["Hallar bordes", "Tracé des contours", "Konturen finden", "Localizar bordas", "Trova bordi"]),
    (
        "Glowing Edges",
        ["Bordes resplandecientes", "Contours lumineux", "Leuchtende Konturen", "Bordas brilhantes", "Bordi luminosi"],
    ),
    ("Oil Paint", ["Pintura al óleo", "Peinture à l'huile", "Ölfarbe", "Pintura a óleo", "Pittura a olio"]),
    ("Solarize", ["Solarizar", "Solarisation", "Solarisation", "Solarizar", "Solarizza"]),
    ("Clouds", ["Nubes", "Nuages", "Wolken", "Nuvens", "Nuvole"]),
    ("Vignette", ["Viñeta", "Vignetage", "Vignette", "Vinheta", "Vignettatura"]),
    ("Lens Flare", ["Destello", "Halo", "Blendenflecke", "Reflexo de lente", "Riflesso lente"]),
    (
        "Liquify (Forward Warp)",
        [
            "Licuar (deformar hacia delante)",
            "Fluidité (déformation avant)",
            "Verflüssigen (Vorwärts-Krümmen)",
            "Dissolver (deformação para a frente)",
            "Fluidifica (alterazione in avanti)",
        ],
    ),
    // View
    ("Zoom In", ["Aumentar", "Zoom avant", "Einzoomen", "Mais zoom", "Aumenta zoom"]),
    ("Zoom Out", ["Reducir", "Zoom arrière", "Auszoomen", "Menos zoom", "Riduci zoom"]),
    ("Fit on Screen", ["Encajar en pantalla", "Taille écran", "Ganzes Bild", "Ajustar na tela", "Adatta allo schermo"]),
    ("Pixel Grid", ["Cuadrícula de píxeles", "Grille de pixels", "Pixelraster", "Grade de pixels", "Griglia pixel"]),
    ("Rulers", ["Reglas", "Règles", "Lineale", "Réguas", "Righelli"]),
    ("Guides", ["Guías", "Repères", "Hilfslinien", "Guias", "Guide"]),
    ("Grid", ["Cuadrícula", "Grille", "Raster", "Grade", "Griglia"]),
    ("Snap", ["Ajustar", "Magnétisme", "Ausrichten", "Encaixar", "Allinea"]),
    ("New Guide", ["Nueva guía", "Nouveau repère", "Neue Hilfslinie", "Nova guia", "Nuova guida"]),
    ("Clear Guides", ["Borrar guías", "Effacer les repères", "Hilfslinien löschen", "Limpar guias", "Cancella guide"]),
    (
        "Selection Edges",
        ["Bordes de selección", "Contours de sélection", "Auswahlkanten", "Bordas da seleção", "Bordi selezione"],
    ),
    (
        "Proof Setup",
        ["Ajuste de prueba", "Format d'épreuve", "Proof einrichten", "Configuração de prova", "Imposta prova"],
    ),
    ("Proof Colors", ["Colores de prueba", "Couleurs d'épreuve", "Farbproof", "Cores de prova", "Colori di prova"]),
    (
        "Gamut Warning",
        ["Aviso de gama", "Couleurs non imprimables", "Farbumfang-Warnung", "Aviso de gama", "Avviso gamma"],
    ),
    (
        "Toggle Light / Dark",
        [
            "Alternar claro/oscuro",
            "Basculer clair/sombre",
            "Hell/Dunkel umschalten",
            "Alternar claro/escuro",
            "Alterna chiaro/scuro",
        ],
    ),
    (
        "Search Commands",
        ["Buscar comandos", "Rechercher des commandes", "Befehle suchen", "Pesquisar comandos", "Cerca comandi"],
    ),
    // Help
    (
        "About Photo Studio",
        [
            "Acerca de Photo Studio",
            "À propos de Photo Studio",
            "Über Photo Studio",
            "Sobre o Photo Studio",
            "Informazioni su Photo Studio",
        ],
    ),
    (
        "Keyboard Shortcuts",
        [
            "Métodos abreviados de teclado",
            "Raccourcis clavier",
            "Tastaturbefehle",
            "Atalhos de teclado",
            "Scelte rapide da tastiera",
        ],
    ),
    (
        "Photo Studio Website",
        [
            "Sitio web de Photo Studio",
            "Site web de Photo Studio",
            "Photo Studio-Website",
            "Site do Photo Studio",
            "Sito web di Photo Studio",
        ],
    ),
    // Tools
    ("Move", ["Mover", "Déplacement", "Verschieben", "Mover", "Sposta"]),
    (
        "Rectangular Marquee",
        [
            "Marco rectangular",
            "Rectangle de sélection",
            "Auswahlrechteck",
            "Letreiro retangular",
            "Selezione rettangolare",
        ],
    ),
    (
        "Elliptical Marquee",
        ["Marco elíptico", "Ellipse de sélection", "Auswahlellipse", "Letreiro elíptico", "Selezione ellittica"],
    ),
    ("Lasso", ["Lazo", "Lasso", "Lasso", "Laço", "Lazo"]),
    ("Polygonal Lasso", ["Lazo poligonal", "Lasso polygonal", "Polygon-Lasso", "Laço poligonal", "Lazo poligonale"]),
    (
        "Magnetic Lasso",
        ["Lazo magnético", "Lasso magnétique", "Magnetisches Lasso", "Laço magnético", "Lazo magnetico"],
    ),
    (
        "Object Selection",
        ["Selección de objeto", "Sélection d'objet", "Objektauswahl", "Seleção de objeto", "Selezione oggetto"],
    ),
    (
        "Quick Selection",
        ["Selección rápida", "Sélection rapide", "Schnellauswahl", "Seleção rápida", "Selezione rapida"],
    ),
    ("Magic Wand", ["Varita mágica", "Baguette magique", "Zauberstab", "Varinha mágica", "Bacchetta magica"]),
    ("Eyedropper", ["Cuentagotas", "Pipette", "Pipette", "Conta-gotas", "Contagocce"]),
    ("Ruler", ["Regla", "Règle", "Linealwerkzeug", "Régua", "Righello"]),
    ("Note", ["Nota", "Note", "Notiz", "Nota", "Nota"]),
    ("Count", ["Recuento", "Comptage", "Zählung", "Contagem", "Conteggio"]),
    ("Slice", ["Sector", "Tranche", "Slice", "Fatia", "Sezione"]),
    (
        "Spot Healing Brush",
        [
            "Pincel corrector puntual",
            "Correcteur localisé",
            "Bereichsreparatur-Pinsel",
            "Pincel de recuperação para manchas",
            "Pennello correttivo al volo",
        ],
    ),
    (
        "Healing Brush",
        ["Pincel corrector", "Correcteur", "Reparatur-Pinsel", "Pincel de recuperação", "Pennello correttivo"],
    ),
    ("Remove", ["Quitar", "Suppression", "Entfernen", "Remover", "Rimozione"]),
    ("Patch", ["Parche", "Pièce", "Ausbessern", "Remendo", "Toppa"]),
    (
        "Content-Aware Move",
        [
            "Movimiento con detección de contenido",
            "Déplacement basé sur le contenu",
            "Inhaltsbasiert verschieben",
            "Movimento sensível ao conteúdo",
            "Spostamento in base al contenuto",
        ],
    ),
    ("Red Eye", ["Pupilas rojas", "Yeux rouges", "Rote-Augen", "Olhos vermelhos", "Occhi rossi"]),
    ("Brush", ["Pincel", "Pinceau", "Pinsel", "Pincel", "Pennello"]),
    ("Pencil", ["Lápiz", "Crayon", "Buntstift", "Lápis", "Matita"]),
    (
        "Mixer Brush",
        ["Pincel mezclador", "Pinceau mélangeur", "Misch-Pinsel", "Pincel misturador", "Pennello miscela colori"],
    ),
    ("Clone Stamp", ["Tampón de clonar", "Tampon de duplication", "Kopierstempel", "Carimbo", "Timbro clone"]),
    (
        "Pattern Stamp",
        ["Tampón de motivo", "Tampon de motif", "Musterstempel", "Carimbo de padrão", "Timbro con pattern"],
    ),
    (
        "History Brush",
        ["Pincel de historia", "Forme d'historique", "Protokoll-Pinsel", "Pincel do histórico", "Pennello storia"],
    ),
    ("Eraser", ["Borrador", "Gomme", "Radiergummi", "Borracha", "Gomma"]),
    (
        "Background Eraser",
        [
            "Borrador de fondos",
            "Gomme d'arrière-plan",
            "Hintergrund-Radiergummi",
            "Borracha de plano de fundo",
            "Gomma per sfondo",
        ],
    ),
    ("Magic Eraser", ["Borrador mágico", "Gomme magique", "Magischer Radiergummi", "Borracha mágica", "Gomma magica"]),
    ("Paint Bucket", ["Bote de pintura", "Pot de peinture", "Füllwerkzeug", "Lata de tinta", "Secchiello"]),
    ("Smudge", ["Dedo", "Doigt", "Wischfinger", "Borrar", "Sfumino"]),
    ("Dodge", ["Sobreexponer", "Densité -", "Abwedler", "Subexposição", "Scherma"]),
    ("Burn", ["Subexponer", "Densité +", "Nachbelichter", "Superexposição", "Brucia"]),
    ("Sponge", ["Esponja", "Éponge", "Schwamm", "Esponja", "Spugna"]),
    ("Type", ["Texto", "Texte", "Text", "Texto", "Testo"]),
    ("Pen", ["Pluma", "Plume", "Zeichenstift", "Caneta", "Penna"]),
    (
        "Path Selection",
        ["Selección de trazado", "Sélection de tracé", "Pfadauswahl", "Seleção de demarcador", "Selezione tracciato"],
    ),
    (
        "Direct Selection",
        ["Selección directa", "Sélection directe", "Direktauswahl", "Seleção direta", "Selezione diretta"],
    ),
    ("Rectangle", ["Rectángulo", "Rectangle", "Rechteck", "Retângulo", "Rettangolo"]),
    (
        "Rounded Rectangle",
        [
            "Rectángulo redondeado",
            "Rectangle arrondi",
            "Abgerundetes Rechteck",
            "Retângulo arredondado",
            "Rettangolo arrotondato",
        ],
    ),
    ("Ellipse", ["Elipse", "Ellipse", "Ellipse", "Elipse", "Ellisse"]),
    ("Polygon", ["Polígono", "Polygone", "Polygon", "Polígono", "Poligono"]),
    ("Star", ["Estrella", "Étoile", "Stern", "Estrela", "Stella"]),
    ("Line", ["Línea", "Trait", "Linie", "Linha", "Linea"]),
    ("Triangle", ["Triángulo", "Triangle", "Dreieck", "Triângulo", "Triangolo"]),
    ("Heart", ["Corazón", "Cœur", "Herz", "Coração", "Cuore"]),
    ("Arrow", ["Flecha", "Flèche", "Pfeil", "Seta", "Freccia"]),
    ("Speech Bubble", ["Bocadillo", "Bulle", "Sprechblase", "Balão de fala", "Fumetto"]),
    ("Checkmark", ["Marca de verificación", "Coche", "Häkchen", "Marca de seleção", "Segno di spunta"]),
    ("Hand", ["Mano", "Main", "Hand", "Mão", "Mano"]),
    ("Rotate View", ["Rotar vista", "Rotation de l'affichage", "Ansichtdrehung", "Girar visualização", "Ruota vista"]),
    ("Zoom", ["Zoom", "Zoom", "Zoom", "Zoom", "Zoom"]),
    ("Center", ["Centro", "Centre", "Mitte", "Centro", "Centro"]),
    // Panels and buttons
    ("Navigator", ["Navegador", "Navigation", "Navigator", "Navegador", "Navigatore"]),
    ("Histogram", ["Histograma", "Histogramme", "Histogramm", "Histograma", "Istogramma"]),
    ("Color", ["Color", "Couleur", "Farbe", "Cor", "Colore"]),
    ("Actions", ["Acciones", "Scripts", "Aktionen", "Ações", "Azioni"]),
    ("Properties", ["Propiedades", "Propriétés", "Eigenschaften", "Propriedades", "Proprietà"]),
    ("Paths", ["Trazados", "Tracés", "Pfade", "Demarcadores", "Tracciati"]),
    ("Channels", ["Canales", "Couches", "Kanäle", "Canais", "Canali"]),
    ("History", ["Historia", "Historique", "Protokoll", "Histórico", "Storia"]),
    ("Layers", ["Capas", "Calques", "Ebenen", "Camadas", "Livelli"]),
    ("OK", ["OK", "OK", "OK", "OK", "OK"]),
    ("Cancel", ["Cancelar", "Annuler", "Abbrechen", "Cancelar", "Annulla"]),
    ("Reset", ["Restaurar", "Réinitialiser", "Zurücksetzen", "Redefinir", "Ripristina"]),
    ("Done", ["Hecho", "Terminé", "Fertig", "Concluído", "Fine"]),
    ("Opacity", ["Opacidad", "Opacité", "Deckkraft", "Opacidade", "Opacità"]),
    ("Size", ["Tamaño", "Taille", "Größe", "Tamanho", "Dimensione"]),
    ("Hardness", ["Dureza", "Dureté", "Härte", "Dureza", "Durezza"]),
    ("Mode", ["Modo", "Mode", "Modus", "Modo", "Metodo"]),
    ("Flow", ["Flujo", "Flux", "Fluss", "Fluxo", "Flusso"]),
    ("layer|Fill", ["Relleno", "Fond", "Fläche", "Preenchimento", "Riempimento"]),
    ("Lock", ["Bloq.", "Verrou", "Fixieren", "Bloquear", "Blocca"]),
    ("Swap", ["Intercambiar", "Permuter", "Tauschen", "Trocar", "Inverti"]),
    ("Default", ["Por defecto", "Par défaut", "Standard", "Padrão", "Predefinito"]),
    (
        "Align to Selection",
        [
            "Alinear con selección",
            "Aligner sur la sélection",
            "An Auswahl ausrichten",
            "Alinhar à seleção",
            "Allinea alla selezione",
        ],
    ),
    (
        "Align to Canvas",
        [
            "Alinear con lienzo",
            "Aligner sur la zone de travail",
            "An Arbeitsfläche ausrichten",
            "Alinhar à tela",
            "Allinea al quadro",
        ],
    ),
    // Layer kinds
    ("Pixel", ["Píxeles", "Pixels", "Pixel", "Pixels", "Pixel"]),
    ("Shape", ["Forma", "Forme", "Form", "Forma", "Forma"]),
    ("Adjustment", ["Ajuste", "Réglage", "Einstellung", "Ajuste", "Regolazione"]),
    ("Group", ["Grupo", "Groupe", "Gruppe", "Grupo", "Gruppo"]),
    (
        "Smart Object",
        ["Objeto inteligente", "Objet dynamique", "Smartobjekt", "Objeto inteligente", "Oggetto avanzato"],
    ),
    // Blend modes
    ("Normal", ["Normal", "Normal", "Normal", "Normal", "Normale"]),
    ("Dissolve", ["Disolver", "Fondu", "Sprenkeln", "Dissolver", "Dissolvi"]),
    ("Darken", ["Oscurecer", "Obscurcir", "Abdunkeln", "Escurecer", "Scurisci"]),
    ("Multiply", ["Multiplicar", "Produit", "Multiplizieren", "Multiplicação", "Moltiplica"]),
    (
        "Color Burn",
        [
            "Subexposición de color",
            "Densité couleur +",
            "Farbig nachbelichten",
            "Superexposição de cores",
            "Brucia colore",
        ],
    ),
    (
        "Linear Burn",
        [
            "Subexposición lineal",
            "Densité linéaire +",
            "Linear nachbelichten",
            "Superexposição linear",
            "Brucia lineare",
        ],
    ),
    (
        "Darker Color",
        ["Color más oscuro", "Couleur plus foncée", "Dunklere Farbe", "Cor mais escura", "Colore più scuro"],
    ),
    ("Lighten", ["Aclarar", "Éclaircir", "Aufhellen", "Clarear", "Schiarisci"]),
    ("Screen", ["Trama", "Superposition", "Negativ multiplizieren", "Tela", "Scolora"]),
    (
        "Color Dodge",
        ["Sobreexposición de color", "Densité couleur -", "Farbig abwedeln", "Subexposição de cores", "Colore scherma"],
    ),
    (
        "Linear Dodge (Add)",
        [
            "Sobreexposición lineal (añadir)",
            "Densité linéaire - (ajout)",
            "Linear abwedeln (Addieren)",
            "Subexposição linear (adicionar)",
            "Scherma lineare (aggiungi)",
        ],
    ),
    (
        "Lighter Color",
        ["Color más claro", "Couleur plus claire", "Hellere Farbe", "Cor mais clara", "Colore più chiaro"],
    ),
    ("Overlay", ["Superponer", "Incrustation", "Ineinanderkopieren", "Sobrepor", "Sovrapponi"]),
    ("Soft Light", ["Luz suave", "Lumière tamisée", "Weiches Licht", "Luz suave", "Luce soffusa"]),
    ("Hard Light", ["Luz fuerte", "Lumière crue", "Hartes Licht", "Luz direta", "Luce intensa"]),
    ("Vivid Light", ["Luz intensa", "Lumière vive", "Strahlendes Licht", "Luz brilhante", "Luce vivida"]),
    ("Linear Light", ["Luz lineal", "Lumière linéaire", "Lineares Licht", "Luz linear", "Luce lineare"]),
    ("Pin Light", ["Luz focal", "Point lumineux", "Lichtpunkt", "Luz do pino", "Luce puntiforme"]),
    ("Hard Mix", ["Mezcla definida", "Mélange maximal", "Hart mischen", "Mistura sólida", "Miscela solida"]),
    ("Difference", ["Diferencia", "Différence", "Differenz", "Diferença", "Differenza"]),
    ("Exclusion", ["Exclusión", "Exclusion", "Ausschluss", "Exclusão", "Esclusione"]),
    ("Subtract", ["Restar", "Soustraction", "Subtrahieren", "Subtrair", "Sottrai"]),
    ("Divide", ["Dividir", "Division", "Dividieren", "Dividir", "Dividi"]),
    ("Hue", ["Tono", "Teinte", "Farbton", "Matiz", "Tonalità"]),
    ("Saturation", ["Saturación", "Saturation", "Sättigung", "Saturação", "Saturazione"]),
    ("Luminosity", ["Luminosidad", "Luminosité", "Luminanz", "Luminosidade", "Luminosità"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_with_fallbacks() {
        set(Lang::German);
        assert_eq!(tr("Layer"), "Ebene");
        assert_eq!(tr("Save As…"), "Speichern unter…", "ellipsis kept");
        assert_eq!(tr("Filter › Blur › Gaussian Blur"), "Filter › Weichzeichnungsfilter › Gaußscher Weichzeichner");
        assert_eq!(tr("Something new"), "Something new", "falls back to English");
        set(Lang::English);
        assert_eq!(tr("Layer"), "Layer");
        assert_eq!(Lang::from_locale("pt_BR"), Lang::Portuguese);
        // Every entry is translated (no empty strings) and keys are unique.
        assert_eq!(TABLE.len(), ENTRIES.len() + crate::i18n_more::MORE.len(), "duplicate keys");
        assert!(ENTRIES.iter().chain(crate::i18n_more::MORE).all(|(_, t)| t.iter().all(|s| !s.is_empty())));
    }
}
