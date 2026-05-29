/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export default {
  sidebar: {
    projects: "Progetti",
    pages: "Pagine",
    new_work_item: "Nuovo elemento di lavoro",
    home: "Home",
    your_work: "Il tuo lavoro",
    inbox: "Posta in arrivo",
    workspace: "workspace",
    views: "Visualizzazioni",
    analytics: "Analisi",
    work_items: "Elementi di lavoro",
    cycles: "Cicli",
    modules: "Moduli",
    intake: "Intake",
    drafts: "Bozze",
    favorites: "Preferiti",
    pro: "Pro",
    upgrade: "Aggiorna",
    stickies: "Stickies",
    prompts: "Prompts",
  },
  auth: {
    common: {
      email: {
        label: "Email",
        placeholder: "nome@azienda.com",
        errors: {
          required: "Email è obbligatoria",
          invalid: "Email non valida",
        },
      },
      password: {
        label: "Password",
        set_password: "Imposta una password",
        placeholder: "Inserisci la password",
        confirm_password: {
          label: "Conferma password",
          placeholder: "Conferma password",
        },
        current_password: {
          label: "Password attuale",
        },
        new_password: {
          label: "Nuova password",
          placeholder: "Inserisci nuova password",
        },
        change_password: {
          label: {
            default: "Cambia password",
            submitting: "Cambiando password",
          },
        },
        errors: {
          match: "Le password non corrispondono",
          empty: "Per favore inserisci la tua password",
          length: "La lunghezza della password deve essere superiore a 8 caratteri",
          strength: {
            weak: "La password è debole",
            strong: "La password è forte",
          },
        },
        submit: "Imposta password",
        toast: {
          change_password: {
            success: {
              title: "Successo!",
              message: "Password cambiata con successo.",
            },
            error: {
              title: "Errore!",
              message: "Qualcosa è andato storto. Per favore riprova.",
            },
          },
          error: {
            title: "",
            message: "",
          },
        },
      },
      unique_code: {
        label: "Codice unico",
        placeholder: "123456",
        paste_code: "Incolla il codice inviato alla tua email",
        requesting_new_code: "Richiesta di nuovo codice",
        sending_code: "Invio codice",
      },
      already_have_an_account: "Hai già un account?",
      login: "Accedi",
      create_account: "Crea un account",
      new_to_pi_dash: "Nuovo su Pi Dash?",
      back_to_sign_in: "Torna al login",
      resend_in: "Reinvia in {seconds} secondi",
      sign_in_with_unique_code: "Accedi con codice unico",
      forgot_password: "Hai dimenticato la password?",
    },
    sign_up: {
      header: {
        label: "Crea un account per iniziare a gestire il lavoro con il tuo team.",
        step: {
          email: {
            header: "Registrati",
            sub_header: "",
          },
          password: {
            header: "Registrati",
            sub_header: "Registrati utilizzando una combinazione email-password.",
          },
          unique_code: {
            header: "Registrati",
            sub_header: "Registrati utilizzando un codice unico inviato all'indirizzo email sopra.",
          },
        },
      },
      errors: {
        password: {
          strength: "Prova a impostare una password forte per procedere",
        },
      },
    },
    sign_in: {
      header: {
        label: "Accedi per iniziare a gestire il lavoro con il tuo team.",
        step: {
          email: {
            header: "Accedi o registrati",
            sub_header: "",
          },
          password: {
            header: "Accedi o registrati",
            sub_header: "Usa la tua combinazione email-password per accedere.",
          },
          unique_code: {
            header: "Accedi o registrati",
            sub_header: "Accedi utilizzando un codice unico inviato all'indirizzo email sopra.",
          },
        },
      },
    },
    forgot_password: {
      title: "Reimposta la tua password",
      description:
        "Inserisci l'indirizzo email verificato del tuo account utente e ti invieremo un link per reimpostare la password.",
      email_sent: "Abbiamo inviato il link di reimpostazione al tuo indirizzo email",
      send_reset_link: "Invia link di reimpostazione",
      errors: {
        smtp_not_enabled:
          "Vediamo che il tuo amministratore non ha abilitato SMTP, non saremo in grado di inviare un link di reimpostazione della password",
      },
      toast: {
        success: {
          title: "Email inviata",
          message:
            "Controlla la tua inbox per un link per reimpostare la tua password. Se non appare entro pochi minuti, controlla la tua cartella spam.",
        },
        error: {
          title: "Errore!",
          message: "Qualcosa è andato storto. Per favore riprova.",
        },
      },
    },
    reset_password: {
      title: "Imposta nuova password",
      description: "Proteggi il tuo account con una password forte",
    },
    set_password: {
      title: "Proteggi il tuo account",
      description: "Impostare una password ti aiuta a accedere in modo sicuro",
    },
    sign_out: {
      toast: {
        error: {
          title: "Errore!",
          message: "Impossibile disconnettersi. Per favore riprova.",
        },
      },
    },
  },
  submit: "Conferma",
  cancel: "Annulla",
  loading: "Caricamento",
  error: "Errore",
  success: "Successo",
  warning: "Avviso",
  info: "Informazioni",
  close: "Chiudi",
  yes: "Sì",
  no: "No",
  ok: "OK",
  name: "Nome",
  description: "Descrizione",
  search: "Cerca",
  add_member: "Aggiungi membro",
  adding_members: "Aggiungendo membri",
  remove_member: "Rimuovi membro",
  add_members: "Aggiungi membri",
  adding_member: "Aggiungendo membro",
  remove_members: "Rimuovi membri",
  add: "Aggiungi",
  adding: "Aggiungendo",
  remove: "Rimuovi",
  add_new: "Aggiungi nuovo",
  remove_selected: "Rimuovi selezionati",
  first_name: "Nome",
  last_name: "Cognome",
  email: "Email",
  display_name: "Nome visualizzato",
  role: "Ruolo",
  timezone: "Fuso orario",
  avatar: "Avatar",
  cover_image: "Immagine di copertina",
  cover_image_upload_skipped: "Cover image upload skipped — using a default cover.",
  integrations: "Integrations",
  github: "GitHub",
  bind: "Bind",
  bound: "Bound",
  edit_disabled_for_external_import_issue: "edit disabled for external import issue",
  git_repository_url_required: "Enter a Git repository URL first.",
  git_repository_url_bind_hint:
    "Bind verifies the URL with GitHub and links this project to that repository. Only github.com URLs are supported. The URL is saved only when you click Bind.",
  password: "Password",
  change_cover: "Cambia copertina",
  language: "Lingua",
  saving: "Salvataggio in corso",
  save_changes: "Salva modifiche",
  deactivate_account: "Disattiva account",
  deactivate_account_description:
    "Disattivando un account, tutti i dati e le risorse al suo interno verranno rimossi definitivamente e non potranno essere recuperati.",
  profile_settings: "Impostazioni del profilo",
  your_account: "Il tuo account",
  security: "Sicurezza",
  activity: "Attività",
  appearance: "Aspetto",
  notifications: "Notifiche",
  workspaces: "Spazi di lavoro",
  create_workspace: "Crea spazio di lavoro",
  invitations: "Inviti",
  summary: "Riepilogo",
  assigned: "Assegnato",
  created: "Creato",
  subscribed: "Iscritto",
  you_do_not_have_the_permission_to_access_this_page: "Non hai il permesso di accedere a questa pagina.",
  something_went_wrong_please_try_again: "Qualcosa è andato storto. Per favore, riprova.",
  load_more: "Carica di più",
  select_or_customize_your_interface_color_scheme: "Seleziona o personalizza lo schema dei colori dell'interfaccia.",
  theme: "Tema",
  system_preference: "Preferenza di sistema",
  light: "Chiaro",
  dark: "Scuro",
  light_contrast: "Contrasto elevato chiaro",
  dark_contrast: "Contrasto elevato scuro",
  custom: "Tema personalizzato",
  select_your_theme: "Seleziona il tuo tema",
  customize_your_theme: "Personalizza il tuo tema",
  background_color: "Colore di sfondo",
  text_color: "Colore del testo",
  primary_color: "Colore primario (Tema)",
  sidebar_background_color: "Colore di sfondo della barra laterale",
  sidebar_text_color: "Colore del testo della barra laterale",
  set_theme: "Imposta tema",
  enter_a_valid_hex_code_of_6_characters: "Inserisci un codice esadecimale valido di 6 caratteri",
  background_color_is_required: "Il colore di sfondo è obbligatorio",
  text_color_is_required: "Il colore del testo è obbligatorio",
  primary_color_is_required: "Il colore primario è obbligatorio",
  sidebar_background_color_is_required: "Il colore di sfondo della barra laterale è obbligatorio",
  sidebar_text_color_is_required: "Il colore del testo della barra laterale è obbligatorio",
  updating_theme: "Aggiornamento del tema in corso",
  theme_updated_successfully: "Tema aggiornato con successo",
  failed_to_update_the_theme: "Impossibile aggiornare il tema",
  email_notifications: "Notifiche via email",
  stay_in_the_loop_on_issues_you_are_subscribed_to_enable_this_to_get_notified:
    "Rimani aggiornato sugli elementi di lavoro a cui sei iscritto. Abilita questa opzione per ricevere notifiche.",
  email_notification_setting_updated_successfully: "Impostazioni delle notifiche email aggiornate con successo",
  failed_to_update_email_notification_setting: "Impossibile aggiornare le impostazioni delle notifiche email",
  notify_me_when: "Avvisami quando",
  property_changes: "Modifiche alle proprietà",
  property_changes_description:
    "Avvisami quando le proprietà degli elementi di lavoro, come assegnatari, priorità, stime o altro, cambiano.",
  state_change: "Cambio di stato",
  state_change_description: "Avvisami quando l'elemento di lavoro passa a uno stato diverso",
  issue_completed: "Elemento di lavoro completato",
  issue_completed_description: "Avvisami solo quando un elemento di lavoro è completato",
  comments: "Commenti",
  comments_description: "Avvisami quando qualcuno lascia un commento sull'elemento di lavoro",
  mentions: "Menzioni",
  mentions_description: "Avvisami solo quando qualcuno mi menziona nei commenti o nella descrizione",
  old_password: "Vecchia password",
  general_settings: "Impostazioni generali",
  sign_out: "Esci",
  signing_out: "Uscita in corso",
  active_cycles: "Cicli attivi",
  active_cycles_description:
    "Monitora i cicli attraverso i progetti, segui gli elementi di lavoro ad alta priorità e analizza i cicli che necessitano attenzione.",
  on_demand_snapshots_of_all_your_cycles: "Snapshot on-demand di tutti i tuoi cicli",
  upgrade: "Aggiorna",
  "10000_feet_view": "Vista panoramica (10.000 piedi) di tutti i cicli attivi.",
  "10000_feet_view_description":
    "Effettua uno zoom indietro per vedere i cicli in esecuzione in tutti i tuoi progetti contemporaneamente, invece di passare da un ciclo all'altro in ogni progetto.",
  get_snapshot_of_each_active_cycle: "Ottieni uno snapshot di ogni ciclo attivo.",
  get_snapshot_of_each_active_cycle_description:
    "Monitora metriche di alto livello per tutti i cicli attivi, osserva il loro stato di avanzamento e valuta la portata rispetto alle scadenze.",
  compare_burndowns: "Confronta i burndown.",
  compare_burndowns_description:
    "Monitora le prestazioni di ciascun team con una rapida occhiata al report del burndown di ogni ciclo.",
  quickly_see_make_or_break_issues: "Visualizza rapidamente gli elementi di lavoro critici.",
  quickly_see_make_or_break_issues_description:
    "Visualizza in anteprima gli elementi di lavoro ad alta priorità per ogni ciclo in base alle scadenze. Vedi tutti con un solo clic.",
  zoom_into_cycles_that_need_attention: "Zoom sui cicli che richiedono attenzione.",
  zoom_into_cycles_that_need_attention_description:
    "Esamina lo stato di ogni ciclo che non rispetta le aspettative con un clic.",
  stay_ahead_of_blockers: "Anticipa gli ostacoli.",
  stay_ahead_of_blockers_description:
    "Individua le sfide tra i progetti e visualizza le dipendenze inter-cicliche non evidenti in altre viste.",
  analytics: "Analisi",
  workspace_invites: "Inviti allo spazio di lavoro",
  enter_god_mode: "Entra in modalità dio",
  workspace_logo: "Logo dello spazio di lavoro",
  new_issue: "Nuovo elemento di lavoro",
  your_work: "Il tuo lavoro",
  drafts: "Bozze",
  projects: "Progetti",
  views: "Visualizzazioni",
  workspace: "Spazio di lavoro",
  archives: "Archivi",
  settings: "Impostazioni",
  failed_to_move_favorite: "Impossibile spostare il preferito",
  favorites: "Preferiti",
  no_favorites_yet: "Nessun preferito ancora",
  create_folder: "Crea cartella",
  new_folder: "Nuova cartella",
  favorite_updated_successfully: "Preferito aggiornato con successo",
  favorite_created_successfully: "Preferito creato con successo",
  folder_already_exists: "La cartella esiste già",
  folder_name_cannot_be_empty: "Il nome della cartella non può essere vuoto",
  something_went_wrong: "Qualcosa è andato storto",
  failed_to_reorder_favorite: "Impossibile riordinare il preferito",
  favorite_removed_successfully: "Preferito rimosso con successo",
  failed_to_create_favorite: "Impossibile creare il preferito",
  failed_to_rename_favorite: "Impossibile rinominare il preferito",
  project_link_copied_to_clipboard: "Link del progetto copiato negli appunti",
  link_copied: "Link copiato",
  add_project: "Aggiungi progetto",
  create_project: "Crea progetto",
  failed_to_remove_project_from_favorites: "Impossibile rimuovere il progetto dai preferiti. Per favore, riprova.",
  project_created_successfully: "Progetto creato con successo",
  project_created_successfully_description:
    "Progetto creato con successo. Ora puoi iniziare ad aggiungere elementi di lavoro.",
  project_name_already_taken: "Il nome del progetto è già stato utilizzato.",
  project_identifier_already_taken: "L'identificatore del progetto è già stato utilizzato.",
  project_cover_image_alt: "Immagine di copertina del progetto",
  name_is_required: "Il nome è obbligatorio",
  title_should_be_less_than_255_characters: "Il titolo deve contenere meno di 255 caratteri",
  project_name: "Nome del progetto",
  project_id_must_be_at_least_1_character: "L'ID del progetto deve contenere almeno 1 carattere",
  project_id_must_be_at_most_5_characters: "L'ID del progetto deve contenere al massimo 5 caratteri",
  project_id: "ID del progetto",
  project_id_tooltip_content:
    "Ti aiuta a identificare in modo univoco gli elementi di lavoro nel progetto. Massimo 10 caratteri.",
  description_placeholder: "Descrizione",
  only_alphanumeric_non_latin_characters_allowed: "Sono ammessi solo caratteri alfanumerici e non latini.",
  project_id_is_required: "L'ID del progetto è obbligatorio",
  project_id_allowed_char: "Sono ammessi solo caratteri alfanumerici e non latini.",
  project_id_min_char: "L'ID del progetto deve contenere almeno 1 carattere",
  project_id_max_char: "L'ID del progetto deve contenere al massimo 10 caratteri",
  project_description_placeholder: "Inserisci la descrizione del progetto",
  select_network: "Seleziona rete",
  lead: "Responsabile",
  date_range: "Intervallo di date",
  private: "Privato",
  public: "Pubblico",
  accessible_only_by_invite: "Accessibile solo su invito",
  anyone_in_the_workspace_except_guests_can_join: "Chiunque nello spazio di lavoro, tranne gli ospiti, può unirsi",
  creating: "Creazione in corso",
  creating_project: "Creazione del progetto in corso",
  adding_project_to_favorites: "Aggiunta del progetto ai preferiti in corso",
  project_added_to_favorites: "Progetto aggiunto ai preferiti",
  couldnt_add_the_project_to_favorites: "Impossibile aggiungere il progetto ai preferiti. Per favore, riprova.",
  removing_project_from_favorites: "Rimozione del progetto dai preferiti in corso",
  project_removed_from_favorites: "Progetto rimosso dai preferiti",
  couldnt_remove_the_project_from_favorites: "Impossibile rimuovere il progetto dai preferiti. Per favore, riprova.",
  add_to_favorites: "Aggiungi ai preferiti",
  remove_from_favorites: "Rimuovi dai preferiti",
  publish_project: "Pubblica progetto",
  publish: "Pubblica",
  copy_link: "Copia link",
  leave_project: "Lascia progetto",
  join_the_project_to_rearrange: "Unisciti al progetto per riorganizzare",
  drag_to_rearrange: "Trascina per riorganizzare",
  congrats: "Congratulazioni!",
  open_project: "Apri progetto",
  issues: "Elementi di lavoro",
  cycles: "Cicli",
  modules: "Moduli",
  pages: "Pagine",
  intake: "Accoglienza",
  time_tracking: "Tracciamento del tempo",
  work_management: "Gestione del lavoro",
  projects_and_issues: "Progetti ed elementi di lavoro",
  projects_and_issues_description: "Attiva o disattiva queste opzioni per questo progetto.",
  cycles_description:
    "Definisci il tempo di lavoro per progetto e adatta il periodo secondo necessità. Un ciclo può durare 2 settimane, il successivo 1 settimana.",
  modules_description: "Organizza il lavoro in sotto-progetti con responsabili e assegnatari dedicati.",
  views_description:
    "Salva ordinamenti, filtri e opzioni di visualizzazione personalizzati o condividili con il tuo team.",
  pages_description: "Crea e modifica contenuti liberi: appunti, documenti, qualsiasi cosa.",
  intake_description:
    "Consenti ai non membri di segnalare bug, feedback e suggerimenti senza interrompere il tuo flusso di lavoro.",
  time_tracking_description: "Registra il tempo trascorso su elementi di lavoro e progetti.",
  work_management_description: "Gestisci il tuo lavoro e i tuoi progetti con facilità.",
  documentation: "Documentazione",
  contact_sales: "Contatta le vendite",
  hyper_mode: "Modalità Hyper",
  keyboard_shortcuts: "Scorciatoie da tastiera",
  whats_new: "Novità?",
  version: "Versione",
  we_are_having_trouble_fetching_the_updates: "Stiamo riscontrando problemi nel recuperare gli aggiornamenti.",
  our_changelogs: "i nostri changelog",
  for_the_latest_updates: "per gli ultimi aggiornamenti.",
  please_visit: "Per favore visita",
  docs: "Documentazione",
  full_changelog: "Changelog completo",
  support: "Supporto",
  forum: "Forum",
  powered_by_pi_dash_pages: "Supportato da Pi Dash Pages",
  please_select_at_least_one_invitation: "Seleziona almeno un invito.",
  please_select_at_least_one_invitation_description: "Seleziona almeno un invito per unirti allo spazio di lavoro.",
  we_see_that_someone_has_invited_you_to_join_a_workspace:
    "Abbiamo notato che qualcuno ti ha invitato a unirti a uno spazio di lavoro",
  join_a_workspace: "Unisciti a uno spazio di lavoro",
  we_see_that_someone_has_invited_you_to_join_a_workspace_description:
    "Abbiamo notato che qualcuno ti ha invitato a unirti a uno spazio di lavoro",
  join_a_workspace_description: "Unisciti a uno spazio di lavoro",
  accept_and_join: "Accetta e unisciti",
  go_home: "Vai alla home",
  no_pending_invites: "Nessun invito in sospeso",
  you_can_see_here_if_someone_invites_you_to_a_workspace:
    "Qui puoi vedere se qualcuno ti invita a uno spazio di lavoro",
  back_to_home: "Torna alla home",
  workspace_name: "nome-spazio-di-lavoro",
  deactivate_your_account: "Disattiva il tuo account",
  deactivate_your_account_description:
    "Una volta disattivato, non potrai più essere assegnato a elementi di lavoro né addebitato per il tuo spazio di lavoro. Per riattivare il tuo account, avrai bisogno di un invito a uno spazio di lavoro associato a questo indirizzo email.",
  deactivating: "Disattivazione in corso",
  confirm: "Conferma",
  confirming: "Conferma in corso",
  draft_created: "Bozza creata",
  issue_created_successfully: "Elemento di lavoro creato con successo",
  draft_creation_failed: "Creazione della bozza fallita",
  issue_creation_failed: "Creazione dell'elemento di lavoro fallita",
  draft_issue: "Bozza di elemento di lavoro",
  issue_updated_successfully: "Elemento di lavoro aggiornato con successo",
  issue_could_not_be_updated: "Impossibile aggiornare l'elemento di lavoro",
  create_a_draft: "Crea una bozza",
  save_to_drafts: "Salva nelle bozze",
  save: "Salva",
  update: "Aggiorna",
  updating: "Aggiornamento in corso",
  create_new_issue: "Crea un nuovo elemento di lavoro",
  editor_is_not_ready_to_discard_changes: "L'editor non è pronto per scartare le modifiche",
  failed_to_move_issue_to_project: "Impossibile spostare l'elemento di lavoro nel progetto",
  create_more: "Crea altri",
  add_to_project: "Aggiungi al progetto",
  discard: "Scarta",
  duplicate_issue_found: "Elemento di lavoro duplicato trovato",
  duplicate_issues_found: "Elementi di lavoro duplicati trovati",
  no_matching_results: "Nessun risultato corrispondente",
  title_is_required: "Il titolo è obbligatorio",
  title: "Titolo",
  state: "Stato",
  priority: "Priorità",
  none: "Nessuna",
  urgent: "Urgente",
  high: "Alta",
  medium: "Media",
  low: "Bassa",
  members: "Membri",
  assignee: "Assegnatario",
  assignees: "Assegnatari",
  you: "Tu",
  labels: "Etichette",
  create_new_label: "Crea nuova etichetta",
  start_date: "Data di inizio",
  end_date: "Data di fine",
  due_date: "Scadenza",
  estimate: "Stima",
  change_parent_issue: "Cambia elemento di lavoro principale",
  remove_parent_issue: "Rimuovi elemento di lavoro principale",
  add_parent: "Aggiungi elemento principale",
  loading_members: "Caricamento membri",
  view_link_copied_to_clipboard: "Link di visualizzazione copiato negli appunti.",
  required: "Obbligatorio",
  optional: "Opzionale",
  Cancel: "Annulla",
  edit: "Modifica",
  archive: "Archivia",
  restore: "Ripristina",
  open_in_new_tab: "Apri in una nuova scheda",
  delete: "Elimina",
  deleting: "Eliminazione in corso",
  make_a_copy: "Crea una copia",
  move_to_project: "Sposta nel progetto",
  good: "Buono",
  morning: "Mattina",
  afternoon: "Pomeriggio",
  evening: "Sera",
  show_all: "Mostra tutto",
  show_less: "Mostra meno",
  no_data_yet: "Nessun dato disponibile",
  syncing: "Sincronizzazione in corso",
  add_work_item: "Aggiungi elemento di lavoro",
  advanced_description_placeholder: "Premi '/' per i comandi",
  create_work_item: "Crea elemento di lavoro",
  attachments: "Allegati",
  declining: "Rifiuto in corso",
  declined: "Rifiutato",
  decline: "Rifiuta",
  unassigned: "Non assegnato",
  work_items: "Elementi di lavoro",
  add_link: "Aggiungi link",
  points: "Punti",
  no_assignee: "Nessun assegnatario",
  no_assignees_yet: "Nessun assegnatario ancora",
  no_labels_yet: "Nessuna etichetta ancora",
  ideal: "Ideale",
  current: "Corrente",
  no_matching_members: "Nessun membro corrispondente",
  leaving: "Uscita in corso",
  removing: "Rimozione in corso",
  leave: "Esci",
  refresh: "Aggiorna",
  refreshing: "Aggiornamento in corso",
  refresh_status: "Stato dell'aggiornamento",
  prev: "Precedente",
  next: "Successivo",
  re_generating: "Rigenerazione in corso",
  re_generate: "Rigenera",
  re_generate_key: "Rigenera chiave",
  export: "Esporta",
  member: "{count, plural, one {# membro} other {# membri}}",
  new_password_must_be_different_from_old_password: "La nuova password deve essere diversa dalla password precedente",
  edited: "Modificato",
  bot: "Bot",
  project_view: {
    sort_by: {
      created_at: "Creato il",
      updated_at: "Aggiornato il",
      name: "Nome",
    },
  },
  toast: {
    success: "Successo!",
    error: "Errore!",
  },
  links: {
    toasts: {
      created: {
        title: "Link creato",
        message: "Il link è stato creato con successo",
      },
      not_created: {
        title: "Link non creato",
        message: "Il link non può essere creato",
      },
      updated: {
        title: "Link aggiornato",
        message: "Il link è stato aggiornato con successo",
      },
      not_updated: {
        title: "Link non aggiornato",
        message: "Il link non può essere aggiornato",
      },
      removed: {
        title: "Link rimosso",
        message: "Il link è stato rimosso con successo",
      },
      not_removed: {
        title: "Link non rimosso",
        message: "Il link non può essere rimosso",
      },
    },
  },
  home: {
    empty: {
      quickstart_guide: "La tua guida rapida",
      not_right_now: "Non ora",
      create_project: {
        title: "Crea un progetto",
        description: "La maggior parte delle cose inizia con un progetto in Pi Dash.",
        cta: "Inizia",
      },
      invite_team: {
        title: "Invita il tuo team",
        description: "Collabora, lancia e gestisci insieme ai colleghi.",
        cta: "Invitali",
      },
      configure_workspace: {
        title: "Configura il tuo spazio di lavoro.",
        description: "Attiva o disattiva le funzionalità o personalizza ulteriormente.",
        cta: "Configura questo spazio",
      },
      personalize_account: {
        title: "Rendi Pi Dash tuo.",
        description: "Scegli la tua immagine, i colori e altro.",
        cta: "Personalizza ora",
      },
      widgets: {
        title: "È silenzioso senza widget, attivali",
        description: "Sembra che tutti i tuoi widget siano disattivati. Attivali ora per migliorare la tua esperienza!",
        primary_button: {
          text: "Gestisci widget",
        },
      },
    },
    quick_links: {
      empty: "Salva link a elementi di lavoro che ti servono.",
      add: "Aggiungi link rapido",
      title: "Link rapido",
      title_plural: "Link rapidi",
    },
    recents: {
      title: "Recenti",
      empty: {
        project: "I tuoi progetti recenti appariranno qui una volta visitati.",
        page: "Le tue pagine recenti appariranno qui una volta visitate.",
        issue: "I tuoi elementi di lavoro recenti appariranno qui una volta visitati.",
        default: "Non hai ancora elementi recenti.",
      },
      filters: {
        all: "Tutti",
        projects: "Progetti",
        pages: "Pagine",
        issues: "Elementi di lavoro",
      },
    },
    new_at_pi_dash: {
      title: "Novità su Pi Dash",
    },
    quick_tutorial: {
      title: "Tutorial rapido",
    },
    widget: {
      reordered_successfully: "Widget riordinato con successo.",
      reordering_failed: "Si è verificato un errore durante il riordino del widget.",
    },
    manage_widgets: "Gestisci widget",
    title: "Home",
    star_us_on_github: "Metti una stella su GitHub",
  },
  link: {
    modal: {
      url: {
        text: "URL",
        required: "L'URL non è valido",
        placeholder: "Digita o incolla un URL",
      },
      title: {
        text: "Titolo di visualizzazione",
        placeholder: "Come vorresti che apparisse questo link",
      },
    },
  },
  common: {
    all: "Tutti",
    no_items_in_this_group: "Nessun elemento in questo gruppo",
    drop_here_to_move: "Rilascia qui per spostare",
    states: "Stati",
    state: "Stato",
    state_groups: "Gruppi di stati",
    priority: "Priorità",
    team_project: "Progetto di squadra",
    project: "Progetto",
    cycle: "Ciclo",
    cycles: "Cicli",
    module: "Modulo",
    modules: "Moduli",
    labels: "Etichette",
    assignees: "Assegnatari",
    assignee: "Assegnatario",
    created_by: "Creato da",
    none: "Nessuno",
    link: "Link",
    estimate: "Stima",
    layout: "Layout",
    filters: "Filtri",
    display: "Visualizza",
    load_more: "Carica di più",
    activity: "Attività",
    analytics: "Analisi",
    dates: "Date",
    success: "Successo!",
    something_went_wrong: "Qualcosa è andato storto",
    error: {
      label: "Errore!",
      message: "Si è verificato un errore. Per favore, riprova.",
    },
    group_by: "Raggruppa per",
    epic: "Epic",
    epics: "Epic",
    work_item: "Elemento di lavoro",
    work_items: "Elementi di lavoro",
    sub_work_item: "Sotto-elemento di lavoro",
    add: "Aggiungi",
    warning: "Avviso",
    updating: "Aggiornamento in corso",
    adding: "Aggiunta in corso",
    update: "Aggiorna",
    creating: "Creazione in corso",
    create: "Crea",
    cancel: "Annulla",
    description: "Descrizione",
    title: "Titolo",
    attachment: "Allegato",
    general: "Generale",
    features: "Funzionalità",
    automation: "Automazione",
    project_name: "Nome del progetto",
    project_id: "ID del progetto",
    project_timezone: "Fuso orario del progetto",
    created_on: "Creato il",
    update_project: "Aggiorna progetto",
    identifier_already_exists: "L'identificatore esiste già",
    add_more: "Aggiungi altro",
    defaults: "Predefiniti",
    add_label: "Aggiungi etichetta",
    estimates: "Stime",
    customize_time_range: "Personalizza intervallo di tempo",
    loading: "Caricamento",
    attachments: "Allegati",
    property: "Proprietà",
    properties: "Proprietà",
    parent: "Principale",
    page: "Pagina",
    remove: "Rimuovi",
    archiving: "Archiviazione in corso",
    archive: "Archivia",
    access: {
      public: "Pubblico",
      private: "Privato",
    },
    done: "Fatto",
    sub_work_items: "Sotto-elementi di lavoro",
    comment: "Commento",
    workspace_level: "Livello dello spazio di lavoro",
    order_by: {
      label: "Ordina per",
      manual: "Manuale",
      last_created: "Ultimo creato",
      last_updated: "Ultimo aggiornato",
      start_date: "Data di inizio",
      due_date: "Scadenza",
      asc: "Ascendente",
      desc: "Discendente",
      updated_on: "Aggiornato il",
    },
    sort: {
      asc: "Ascendente",
      desc: "Discendente",
      created_on: "Creato il",
      updated_on: "Aggiornato il",
    },
    comments: "Commenti",
    updates: "Aggiornamenti",
    clear_all: "Pulisci tutto",
    copied: "Copiato!",
    link_copied: "Link copiato!",
    link_copied_to_clipboard: "Link copiato negli appunti",
    copied_to_clipboard: "Link dell'elemento di lavoro copiato negli appunti",
    is_copied_to_clipboard: "Elemento di lavoro copiato negli appunti",
    no_links_added_yet: "Nessun link aggiunto ancora",
    add_link: "Aggiungi link",
    links: "Link",
    go_to_workspace: "Vai allo spazio di lavoro",
    progress: "Progresso",
    optional: "Opzionale",
    join: "Unisciti",
    go_back: "Torna indietro",
    continue: "Continua",
    resend: "Reinvia",
    relations: "Relazioni",
    errors: {
      default: {
        title: "Errore!",
        message: "Qualcosa è andato storto. Per favore, riprova.",
      },
      required: "Questo campo è obbligatorio",
      entity_required: "{entity} è obbligatorio",
      restricted_entity: "{entity} è limitato",
    },
    update_link: "Aggiorna link",
    attach: "Allega",
    create_new: "Crea nuovo",
    add_existing: "Aggiungi esistente",
    type_or_paste_a_url: "Digita o incolla un URL",
    url_is_invalid: "L'URL non è valido",
    display_title: "Titolo di visualizzazione",
    link_title_placeholder: "Come vorresti vedere questo link",
    url: "URL",
    side_peek: "Visualizzazione laterale",
    modal: "Modal",
    full_screen: "Schermo intero",
    close_peek_view: "Chiudi la visualizzazione rapida",
    toggle_peek_view_layout: "Alterna layout della visualizzazione rapida",
    options: "Opzioni",
    duration: "Durata",
    today: "Oggi",
    week: "Settimana",
    month: "Mese",
    quarter: "Trimestre",
    press_for_commands: "Premi '/' per i comandi",
    click_to_add_description: "Clicca per aggiungere una descrizione",
    search: {
      label: "Cerca",
      placeholder: "Digita per cercare",
      no_matches_found: "Nessuna corrispondenza trovata",
      no_matching_results: "Nessun risultato corrispondente",
    },
    actions: {
      edit: "Modifica",
      make_a_copy: "Crea una copia",
      open_in_new_tab: "Apri in una nuova scheda",
      copy_link: "Copia link",
      archive: "Archivia",
      restore: "Ripristina",
      delete: "Elimina",
      remove_relation: "Rimuovi relazione",
      subscribe: "Iscriviti",
      unsubscribe: "Annulla iscrizione",
      clear_sorting: "Cancella ordinamento",
      show_weekends: "Mostra weekend",
      enable: "Abilita",
      disable: "Disabilita",
      copy_markdown: "Copia markdown",
    },
    name: "Nome",
    discard: "Scarta",
    confirm: "Conferma",
    confirming: "Conferma in corso",
    read_the_docs: "Leggi la documentazione",
    default: "Predefinito",
    active: "Attivo",
    enabled: "Abilitato",
    disabled: "Disabilitato",
    mandate: "Obbligo",
    mandatory: "Obbligatorio",
    yes: "Sì",
    no: "No",
    please_wait: "Attendere prego",
    enabling: "Abilitazione in corso",
    disabling: "Disabilitazione in corso",
    beta: "Beta",
    or: "o",
    next: "Successivo",
    back: "Indietro",
    cancelling: "Annullamento in corso",
    configuring: "Configurazione in corso",
    clear: "Pulisci",
    import: "Importa",
    connect: "Connetti",
    authorizing: "Autorizzazione in corso",
    processing: "Elaborazione in corso",
    no_data_available: "Nessun dato disponibile",
    from: "da {name}",
    authenticated: "Autenticato",
    select: "Seleziona",
    upgrade: "Aggiorna",
    add_seats: "Aggiungi postazioni",
    label: "Etichetta",
    priorities: "Priorità",
    projects: "Progetti",
    workspace: "Spazio di lavoro",
    workspaces: "Spazi di lavoro",
    team: "Team",
    teams: "Team",
    entity: "Entità",
    entities: "Entità",
    task: "Attività",
    tasks: "Attività",
    section: "Sezione",
    sections: "Sezioni",
    edit: "Modifica",
    connecting: "Connessione in corso",
    connected: "Connesso",
    disconnect: "Disconnetti",
    disconnecting: "Disconnessione in corso",
    installing: "Installazione in corso",
    install: "Installa",
    reset: "Reimposta",
    live: "Live",
    change_history: "Cronologia modifiche",
    coming_soon: "Prossimamente",
    member: "Membro",
    members: "Membri",
    you: "Tu",
    upgrade_cta: {
      higher_subscription: "Passa a un abbonamento superiore",
      talk_to_sales: "Parla con le vendite",
    },
    category: "Categoria",
    categories: "Categorie",
    saving: "Salvataggio in corso",
    save_changes: "Salva modifiche",
    delete: "Elimina",
    deleting: "Eliminazione in corso",
    pending: "In sospeso",
    invite: "Invita",
    view: "Visualizza",
    deactivated_user: "Utente disattivato",
    apply: "Applica",
    applying: "Applicazione",
    users: "Utenti",
    admins: "Amministratori",
    guests: "Ospiti",
    on_track: "In linea",
    off_track: "Fuori rotta",
    at_risk: "A rischio",
    timeline: "Cronologia",
    completion: "Completamento",
    upcoming: "In arrivo",
    completed: "Completato",
    in_progress: "In corso",
    planned: "Pianificato",
    paused: "In pausa",
    no_of: "N. di {entity}",
    resolved: "Risolto",
    overview: "Panoramica",
  },
  chart: {
    x_axis: "Asse X",
    y_axis: "Asse Y",
    metric: "Metrica",
  },
  form: {
    title: {
      required: "Il titolo è obbligatorio",
      max_length: "Il titolo deve contenere meno di {length} caratteri",
    },
  },
  entity: {
    grouping_title: "Raggruppamento di {entity}",
    priority: "Priorità di {entity}",
    all: "Tutti {entity}",
    drop_here_to_move: "Trascina qui per spostare il {entity}",
    delete: {
      label: "Elimina {entity}",
      success: "{entity} eliminato con successo",
      failed: "Eliminazione di {entity} fallita",
    },
    update: {
      failed: "Aggiornamento di {entity} fallito",
      success: "{entity} aggiornato con successo",
    },
    link_copied_to_clipboard: "Link di {entity} copiato negli appunti",
    fetch: {
      failed: "Errore durante il recupero di {entity}",
    },
    add: {
      success: "{entity} aggiunto con successo",
      failed: "Errore nell'aggiunta di {entity}",
    },
    remove: {
      success: "{entity} rimosso con successo",
      failed: "Errore nella rimozione di {entity}",
    },
  },
  epic: {
    all: "Tutti gli Epic",
    label: "{count, plural, one {Epic} other {Epic}}",
    new: "Nuovo Epic",
    adding: "Aggiungendo Epic",
    create: {
      success: "Epic creato con successo",
    },
    add: {
      press_enter: "Premi 'Invio' per aggiungere un altro Epic",
      label: "Aggiungi Epic",
    },
    title: {
      label: "Titolo Epic",
      required: "Il titolo dell'Epic è obbligatorio.",
    },
  },
  issue: {
    label: "{count, plural, one {Elemento di lavoro} other {Elementi di lavoro}}",
    all: "Tutti gli elementi di lavoro",
    edit: "Modifica elemento di lavoro",
    title: {
      label: "Titolo dell'elemento di lavoro",
      required: "Il titolo dell'elemento di lavoro è obbligatorio.",
    },
    add: {
      press_enter: "Premi 'Invio' per aggiungere un altro elemento di lavoro",
      label: "Aggiungi elemento di lavoro",
      cycle: {
        failed: "Impossibile aggiungere l'elemento di lavoro al ciclo. Per favore, riprova.",
        success: "{count, plural, one {Elemento di lavoro} other {Elementi di lavoro}} aggiunto al ciclo con successo.",
        loading: "Aggiungendo {count, plural, one {elemento di lavoro} other {elementi di lavoro}} al ciclo",
      },
      assignee: "Aggiungi assegnatari",
      start_date: "Aggiungi data di inizio",
      due_date: "Aggiungi scadenza",
      parent: "Aggiungi elemento di lavoro principale",
      sub_issue: "Aggiungi sotto-elemento di lavoro",
      relation: "Aggiungi relazione",
      link: "Aggiungi link",
      existing: "Aggiungi elemento di lavoro esistente",
    },
    remove: {
      label: "Rimuovi elemento di lavoro",
      cycle: {
        loading: "Rimuovendo l'elemento di lavoro dal ciclo",
        success: "Elemento di lavoro rimosso dal ciclo con successo.",
        failed: "Impossibile rimuovere l'elemento di lavoro dal ciclo. Per favore, riprova.",
      },
      module: {
        loading: "Rimuovendo l'elemento di lavoro dal modulo",
        success: "Elemento di lavoro rimosso dal modulo con successo.",
        failed: "Impossibile rimuovere l'elemento di lavoro dal modulo. Per favore, riprova.",
      },
      parent: {
        label: "Rimuovi elemento di lavoro principale",
      },
    },
    new: "Nuovo elemento di lavoro",
    adding: "Aggiunta dell'elemento di lavoro in corso",
    create: {
      success: "Elemento di lavoro creato con successo",
    },
    priority: {
      urgent: "Urgente",
      high: "Alta",
      medium: "Media",
      low: "Bassa",
    },
    display: {
      properties: {
        label: "Visualizza proprietà",
        id: "ID",
        issue_type: "Tipo di elemento di lavoro",
        sub_issue_count: "Numero di sotto-elementi di lavoro",
        attachment_count: "Numero di allegati",
        created_on: "Creato il",
        sub_issue: "Sotto-elemento di lavoro",
        work_item_count: "Conteggio degli elementi di lavoro",
      },
      extra: {
        show_sub_issues: "Mostra sotto-elementi di lavoro",
        show_empty_groups: "Mostra gruppi vuoti",
      },
    },
    layouts: {
      ordered_by_label: "Questo layout è ordinato per",
      list: "Lista",
      kanban: "Schede",
      calendar: "Calendario",
      spreadsheet: "Tabella",
      gantt: "Timeline",
      title: {
        list: "Layout a lista",
        kanban: "Layout a schede",
        calendar: "Layout a calendario",
        spreadsheet: "Layout a tabella",
        gantt: "Layout a timeline",
      },
    },
    states: {
      active: "Attivo",
      backlog: "Backlog",
    },
    comments: {
      placeholder: "Aggiungi commento",
      switch: {
        private: "Passa a commento privato",
        public: "Passa a commento pubblico",
      },
      create: {
        success: "Commento creato con successo",
        error: "Creazione del commento fallita. Per favore, riprova più tardi.",
      },
      update: {
        success: "Commento aggiornato con successo",
        error: "Aggiornamento del commento fallito. Per favore, riprova più tardi.",
      },
      remove: {
        success: "Commento rimosso con successo",
        error: "Rimozione del commento fallita. Per favore, riprova più tardi.",
      },
      upload: {
        error: "Caricamento dell'asset fallito. Per favore, riprova più tardi.",
      },
      copy_link: {
        success: "Link del commento copiato negli appunti",
        error: "Errore durante la copia del link del commento. Riprova più tardi.",
      },
    },
    empty_state: {
      issue_detail: {
        title: "L'elemento di lavoro non esiste",
        description: "L'elemento di lavoro che stai cercando non esiste, è stato archiviato o eliminato.",
        primary_button: {
          text: "Visualizza altri elementi di lavoro",
        },
      },
    },
    sibling: {
      label: "Elementi di lavoro correlati",
    },
    archive: {
      description: "Solo gli elementi di lavoro completati o annullati possono essere archiviati",
      label: "Archivia elemento di lavoro",
      confirm_message:
        "Sei sicuro di voler archiviare l'elemento di lavoro? Tutti gli elementi di lavoro archiviati possono essere ripristinati in seguito.",
      success: {
        label: "Archiviazione riuscita",
        message: "I tuoi archivi sono disponibili negli archivi del progetto.",
      },
      failed: {
        message: "Impossibile archiviare l'elemento di lavoro. Per favore, riprova.",
      },
    },
    restore: {
      success: {
        title: "Ripristino riuscito",
        message: "Il tuo elemento di lavoro è disponibile negli elementi del progetto.",
      },
      failed: {
        message: "Impossibile ripristinare l'elemento di lavoro. Per favore, riprova.",
      },
    },
    relation: {
      relates_to: "Collegato a",
      duplicate: "Duplicato di",
      blocked_by: "Bloccato da",
      blocking: "Blocca",
    },
    copy_link: "Copia link dell'elemento di lavoro",
    delete: {
      label: "Elimina elemento di lavoro",
      error: "Errore nell'eliminazione dell'elemento di lavoro",
    },
    subscription: {
      actions: {
        subscribed: "Iscrizione all'elemento di lavoro avvenuta con successo",
        unsubscribed: "Disiscrizione dall'elemento di lavoro avvenuta con successo",
      },
    },
    select: {
      error: "Seleziona almeno un elemento di lavoro",
      empty: "Nessun elemento di lavoro selezionato",
      add_selected: "Aggiungi gli elementi di lavoro selezionati",
      select_all: "Seleziona tutto",
      deselect_all: "Deseleziona tutto",
    },
    open_in_full_screen: "Apri l'elemento di lavoro a schermo intero",
  },
  attachment: {
    error: "Impossibile allegare il file. Riprova a caricarlo.",
    only_one_file_allowed: "È possibile caricare un solo file alla volta.",
    file_size_limit: "Il file deve essere di {size}MB o meno.",
    drag_and_drop: "Trascina e rilascia ovunque per caricare",
    delete: "Elimina allegato",
  },
  label: {
    select: "Seleziona etichetta",
    create: {
      success: "Etichetta creata con successo",
      failed: "Creazione dell'etichetta fallita",
      already_exists: "L'etichetta esiste già",
      type: "Digita per aggiungere una nuova etichetta",
    },
  },
  sub_work_item: {
    update: {
      success: "Sotto-elemento di lavoro aggiornato con successo",
      error: "Errore nell'aggiornamento del sotto-elemento di lavoro",
    },
    remove: {
      success: "Sotto-elemento di lavoro rimosso con successo",
      error: "Errore nella rimozione del sotto-elemento di lavoro",
    },
    empty_state: {
      sub_list_filters: {
        title: "Non hai sotto-elementi di lavoro che corrispondono ai filtri che hai applicato.",
        description: "Per vedere tutti i sotto-elementi di lavoro, cancella tutti i filtri applicati.",
        action: "Cancella filtri",
      },
      list_filters: {
        title: "Non hai elementi di lavoro che corrispondono ai filtri che hai applicato.",
        description: "Per vedere tutti gli elementi di lavoro, cancella tutti i filtri applicati.",
        action: "Cancella filtri",
      },
    },
  },
  view: {
    label: "{count, plural, one {Visualizzazione} other {Visualizzazioni}}",
    create: {
      label: "Crea visualizzazione",
    },
    update: {
      label: "Aggiorna visualizzazione",
    },
  },
  inbox_issue: {
    status: {
      pending: {
        title: "In sospeso",
        description: "In sospeso",
      },
      declined: {
        title: "Rifiutato",
        description: "Rifiutato",
      },
      snoozed: {
        title: "Snoozed",
        description: "{days, plural, one {# giorno} other {# giorni}} rimanenti",
      },
      accepted: {
        title: "Accettato",
        description: "Accettato",
      },
      duplicate: {
        title: "Duplicato",
        description: "Duplicato",
      },
    },
    modals: {
      decline: {
        title: "Rifiuta elemento di lavoro",
        content: "Sei sicuro di voler rifiutare l'elemento di lavoro {value}?",
      },
      delete: {
        title: "Elimina elemento di lavoro",
        content: "Sei sicuro di voler eliminare l'elemento di lavoro {value}?",
        success: "Elemento di lavoro eliminato con successo",
      },
    },
    errors: {
      snooze_permission: "Solo gli amministratori del progetto possono snoozare/non snoozare gli elementi di lavoro",
      accept_permission: "Solo gli amministratori del progetto possono accettare gli elementi di lavoro",
      decline_permission: "Solo gli amministratori del progetto possono rifiutare gli elementi di lavoro",
    },
    actions: {
      accept: "Accetta",
      decline: "Rifiuta",
      snooze: "Snoozed",
      unsnooze: "Annulla snooze",
      copy: "Copia link dell'elemento di lavoro",
      delete: "Elimina",
      open: "Apri elemento di lavoro",
      mark_as_duplicate: "Segna come duplicato",
      move: "Sposta {value} negli elementi di lavoro del progetto",
    },
    source: {
      "in-app": "nell'app",
    },
    order_by: {
      created_at: "Creato il",
      updated_at: "Aggiornato il",
      id: "ID",
    },
    label: "Accoglienza",
    page_label: "{workspace} - Accoglienza",
    modal: {
      title: "Crea elemento di lavoro per l'accoglienza",
    },
    tabs: {
      open: "Aperto",
      closed: "Chiuso",
    },
    empty_state: {
      sidebar_open_tab: {
        title: "Nessun elemento di lavoro aperto",
        description: "Trova qui gli elementi di lavoro aperti. Crea un nuovo elemento di lavoro.",
      },
      sidebar_closed_tab: {
        title: "Nessun elemento di lavoro chiuso",
        description: "Tutti gli elementi di lavoro, siano essi accettati o rifiutati, possono essere trovati qui.",
      },
      sidebar_filter: {
        title: "Nessun elemento di lavoro corrispondente",
        description:
          "Nessun elemento di lavoro corrisponde al filtro applicato in accoglienza. Crea un nuovo elemento di lavoro.",
      },
      detail: {
        title: "Seleziona un elemento di lavoro per visualizzarne i dettagli.",
      },
    },
  },
  workspace_creation: {
    heading: "Crea il tuo spazio di lavoro",
    subheading: "Per iniziare a usare Pi Dash, devi creare o unirti a uno spazio di lavoro.",
    form: {
      name: {
        label: "Dai un nome al tuo spazio di lavoro",
        placeholder: "Qualcosa di familiare e riconoscibile è sempre meglio.",
      },
      url: {
        label: "Imposta l'URL del tuo spazio di lavoro",
        placeholder: "Digita o incolla un URL",
        edit_slug: "Puoi modificare solo lo slug dell'URL",
      },
      organization_size: {
        label: "Quante persone utilizzeranno questo spazio di lavoro?",
        placeholder: "Seleziona una fascia",
      },
    },
    errors: {
      creation_disabled: {
        title: "Solo l'amministratore dell'istanza può creare spazi di lavoro",
        description:
          "Se conosci l'indirizzo email dell'amministratore dell'istanza, clicca il pulsante qui sotto per contattarlo.",
        request_button: "Richiedi all'amministratore dell'istanza",
      },
      validation: {
        name_alphanumeric:
          "I nomi degli spazi di lavoro possono contenere solo (' '), ('-'), ('_') e caratteri alfanumerici.",
        name_length: "Limita il tuo nome a 80 caratteri.",
        url_alphanumeric: "Gli URL possono contenere solo ('-') e caratteri alfanumerici.",
        url_length: "Limita il tuo URL a 48 caratteri.",
        url_already_taken: "L'URL dello spazio di lavoro è già in uso!",
      },
    },
    request_email: {
      subject: "Richiesta per un nuovo spazio di lavoro",
      body: "Ciao amministratore dell'istanza,\n\nPer favore, crea un nuovo spazio di lavoro con l'URL [/nome-spazio] per [scopo del nuovo spazio].\n\nGrazie,\n{firstName} {lastName}\n{email}",
    },
    button: {
      default: "Crea spazio di lavoro",
      loading: "Creazione dello spazio di lavoro in corso",
    },
    toast: {
      success: {
        title: "Successo",
        message: "Spazio di lavoro creato con successo",
      },
      error: {
        title: "Errore",
        message: "Impossibile creare lo spazio di lavoro. Per favore, riprova.",
      },
    },
  },
  workspace_dashboard: {
    empty_state: {
      general: {
        title: "Panoramica dei tuoi progetti, attività e metriche",
        description:
          "Benvenuto in Pi Dash, siamo entusiasti di averti qui. Crea il tuo primo progetto e traccia i tuoi elementi di lavoro, e questa pagina si trasformerà in uno spazio che ti aiuta a progredire. Gli amministratori vedranno anche elementi che aiutano il team a progredire.",
        primary_button: {
          text: "Crea il tuo primo progetto",
          comic: {
            title: "Tutto inizia con un progetto in Pi Dash",
            description:
              "Un progetto può essere la roadmap di un prodotto, una campagna di marketing o il lancio di una nuova auto.",
          },
        },
      },
    },
  },
  workspace_analytics: {
    label: "Analisi",
    page_label: "{workspace} - Analisi",
    open_tasks: "Totale attività aperte",
    error: "Si è verificato un errore nel recupero dei dati.",
    work_items_closed_in: "Elementi di lavoro chiusi in",
    selected_projects: "Progetti selezionati",
    total_members: "Totale membri",
    total_cycles: "Totale cicli",
    total_modules: "Totale moduli",
    pending_work_items: {
      title: "Elementi di lavoro in sospeso",
      empty_state: "L'analisi degli elementi di lavoro in sospeso dei colleghi apparirà qui.",
    },
    work_items_closed_in_a_year: {
      title: "Elementi di lavoro chiusi in un anno",
      empty_state: "Chiudi gli elementi di lavoro per visualizzare l'analisi sotto forma di grafico.",
    },
    most_work_items_created: {
      title: "Maggiori elementi di lavoro creati",
      empty_state: "I colleghi e il numero di elementi di lavoro creati da loro appariranno qui.",
    },
    most_work_items_closed: {
      title: "Maggiori elementi di lavoro chiusi",
      empty_state: "I colleghi e il numero di elementi di lavoro chiusi da loro appariranno qui.",
    },
    tabs: {
      scope_and_demand: "Ambito e Domanda",
      custom: "Analisi personalizzata",
    },
    empty_state: {
      customized_insights: {
        description: "Gli elementi di lavoro assegnati a te, suddivisi per stato, verranno visualizzati qui.",
        title: "Nessun dato disponibile",
      },
      created_vs_resolved: {
        description: "Gli elementi di lavoro creati e risolti nel tempo verranno visualizzati qui.",
        title: "Nessun dato disponibile",
      },
      project_insights: {
        title: "Nessun dato disponibile",
        description: "Gli elementi di lavoro assegnati a te, suddivisi per stato, verranno visualizzati qui.",
      },
      general: {
        title:
          "Traccia progressi, carichi di lavoro e allocazioni. Individua tendenze, rimuovi blocchi e lavora più velocemente",
        description:
          "Visualizza ambito vs domanda, stime e scope creep. Ottieni prestazioni per membri del team e squadre, assicurandoti che il tuo progetto si svolga nei tempi previsti.",
        primary_button: {
          text: "Inizia il tuo primo progetto",
          comic: {
            title: "Analytics funziona meglio con Cicli + Moduli",
            description:
              "Prima, incornicia i tuoi elementi di lavoro in Cicli e, se possibile, raggruppa gli elementi che si estendono oltre un ciclo in Moduli. Controlla entrambi nella navigazione sinistra.",
          },
        },
      },
    },
    created_vs_resolved: "Creato vs Risolto",
    customized_insights: "Approfondimenti personalizzati",
    backlog_work_items: "{entity} nel backlog",
    active_projects: "Progetti attivi",
    trend_on_charts: "Tendenza nei grafici",
    all_projects: "Tutti i progetti",
    summary_of_projects: "Riepilogo dei progetti",
    project_insights: "Approfondimenti sul progetto",
    started_work_items: "{entity} iniziati",
    total_work_items: "Totale {entity}",
    total_projects: "Progetti totali",
    total_admins: "Totale amministratori",
    total_users: "Totale utenti",
    total_intake: "Entrate totali",
    un_started_work_items: "{entity} non avviati",
    total_guests: "Totale ospiti",
    completed_work_items: "{entity} completati",
    total: "Totale {entity}",
  },
  workspace_projects: {
    label: "{count, plural, one {Progetto} other {Progetti}}",
    create: {
      label: "Aggiungi progetto",
    },
    network: {
      label: "Rete",
      private: {
        title: "Privato",
        description: "Accessibile solo su invito",
      },
      public: {
        title: "Pubblico",
        description: "Chiunque nello spazio di lavoro, tranne gli ospiti, può unirsi",
      },
    },
    error: {
      permission: "Non hai il permesso di eseguire questa azione.",
      cycle_delete: "Impossibile eliminare il ciclo",
      module_delete: "Impossibile eliminare il modulo",
      issue_delete: "Impossibile eliminare l'elemento di lavoro",
    },
    state: {
      backlog: "Backlog",
      unstarted: "Non iniziato",
      started: "Iniziato",
      completed: "Completato",
      cancelled: "Annullato",
    },
    sort: {
      manual: "Manuale",
      name: "Nome",
      created_at: "Data di creazione",
      members_length: "Numero di membri",
    },
    scope: {
      my_projects: "I miei progetti",
      archived_projects: "Archiviati",
    },
    common: {
      months_count: "{months, plural, one {# mese} other {# mesi}}",
    },
    empty_state: {
      general: {
        title: "Nessun progetto attivo",
        description:
          "Considera ogni progetto come la base per un lavoro orientato a obiettivi. I progetti sono dove risiedono Jobs, Cicli e Moduli e, insieme ai tuoi colleghi, ti aiutano a raggiungere quell'obiettivo. Crea un nuovo progetto o filtra per progetti archiviati.",
        primary_button: {
          text: "Inizia il tuo primo progetto",
          comic: {
            title: "Tutto inizia con un progetto in Pi Dash",
            description:
              "Un progetto può essere la roadmap di un prodotto, una campagna di marketing o il lancio di una nuova auto.",
          },
        },
      },
      no_projects: {
        title: "Nessun progetto",
        description: "Per creare elementi di lavoro o gestire il tuo lavoro, devi creare o far parte di un progetto.",
        primary_button: {
          text: "Inizia il tuo primo progetto",
          comic: {
            title: "Tutto inizia con un progetto in Pi Dash",
            description:
              "Un progetto può essere la roadmap di un prodotto, una campagna di marketing o il lancio di una nuova auto.",
          },
        },
      },
      filter: {
        title: "Nessun progetto corrispondente",
        description:
          "Nessun progetto rilevato con i criteri di ricerca corrispondenti. \n Crea un nuovo progetto invece.",
      },
      search: {
        description: "Nessun progetto rilevato con i criteri di ricerca corrispondenti.\nCrea un nuovo progetto invece",
      },
    },
  },
  workspace_views: {
    add_view: "Aggiungi visualizzazione",
    empty_state: {
      "all-issues": {
        title: "Nessun elemento di lavoro nel progetto",
        description:
          "Primo progetto fatto! Ora, suddividi il tuo lavoro in parti tracciabili con gli elementi di lavoro. Andiamo!",
        primary_button: {
          text: "Crea un nuovo elemento di lavoro",
        },
      },
      assigned: {
        title: "Nessun elemento di lavoro ancora",
        description: "Gli elementi di lavoro assegnati a te possono essere tracciati da qui.",
        primary_button: {
          text: "Crea un nuovo elemento di lavoro",
        },
      },
      created: {
        title: "Nessun elemento di lavoro ancora",
        description: "Tutti gli elementi di lavoro creati da te appariranno qui. Tracciali direttamente da qui.",
        primary_button: {
          text: "Crea un nuovo elemento di lavoro",
        },
      },
      subscribed: {
        title: "Nessun elemento di lavoro ancora",
        description: "Iscriviti agli elementi di lavoro che ti interessano, tracciali tutti qui.",
      },
      "custom-view": {
        title: "Nessun elemento di lavoro ancora",
        description: "Gli elementi di lavoro che corrispondono ai filtri, tracciali tutti qui.",
      },
    },
    delete_view: {
      title: "Sei sicuro di voler eliminare questa visualizzazione?",
      content:
        "Se confermi, tutte le opzioni di ordinamento, filtro e visualizzazione + il layout che hai scelto per questa visualizzazione saranno eliminate permanentemente senza possibilità di ripristinarle.",
    },
  },
  account_settings: {
    profile: {
      change_email_modal: {
        title: "Cambia email",
        description: "Inserisci un nuovo indirizzo email per ricevere un link di verifica.",
        toasts: {
          success_title: "Successo!",
          success_message: "Email aggiornata con successo. Accedi di nuovo.",
        },
        form: {
          email: {
            label: "Nuova email",
            placeholder: "Inserisci la tua email",
            errors: {
              required: "L’email è obbligatoria",
              invalid: "L’email non è valida",
              exists: "L’email esiste già. Usane un’altra.",
              validation_failed: "La verifica dell’email non è riuscita. Riprova.",
            },
          },
          code: {
            label: "Codice univoco",
            placeholder: "123456",
            helper_text: "Codice di verifica inviato alla tua nuova email.",
            errors: {
              required: "Il codice univoco è obbligatorio",
              invalid: "Codice di verifica non valido. Riprova.",
            },
          },
        },
        actions: {
          continue: "Continua",
          confirm: "Conferma",
          cancel: "Annulla",
        },
        states: {
          sending: "Invio…",
        },
      },
    },
    activity: {
      heading: "Attività",
      description: "Tieni traccia delle tue azioni e modifiche recenti in tutti i progetti e gli elementi di lavoro.",
    },
    api_tokens: {
      heading: "Token di accesso personali",
      description: "Genera token API sicuri per integrare i tuoi dati con sistemi e applicazioni esterni.",
    },
    notifications: {
      heading: "Notifiche email",
      description:
        "Rimani aggiornato sugli elementi di lavoro a cui sei iscritto. Attiva questa opzione per ricevere notifiche.",
    },
    preferences: {
      heading: "Preferenze",
      description: "Personalizza l'esperienza dell'app in base al tuo modo di lavorare",
    },
  },
  workspace_settings: {
    label: "Impostazioni dello spazio di lavoro",
    page_label: "{workspace} - Impostazioni generali",
    key_created: "Chiave creata",
    copy_key:
      "Copia e salva questa chiave segreta in Pi Dash Pages. Non potrai vederla dopo aver cliccato Chiudi. È stato scaricato un file CSV contenente la chiave.",
    token_copied: "Token copiato negli appunti.",
    settings: {
      general: {
        title: "Generale",
        upload_logo: "Carica logo",
        edit_logo: "Modifica logo",
        name: "Nome dello spazio di lavoro",
        company_size: "Dimensione aziendale",
        url: "URL dello spazio di lavoro",
        workspace_timezone: "Fuso orario dello spazio di lavoro",
        update_workspace: "Aggiorna spazio di lavoro",
        delete_workspace: "Elimina questo spazio di lavoro",
        delete_workspace_description:
          "Eliminando uno spazio di lavoro, tutti i dati e le risorse all'interno di esso verranno rimossi definitivamente e non potranno essere recuperati.",
        delete_btn: "Elimina questo spazio di lavoro",
        delete_modal: {
          title: "Sei sicuro di voler eliminare questo spazio di lavoro?",
          description:
            "Hai un periodo di prova attivo per uno dei nostri piani a pagamento. Per procedere, annulla prima il periodo di prova.",
          dismiss: "Annulla",
          cancel: "Annulla periodo di prova",
          success_title: "Spazio di lavoro eliminato.",
          success_message: "Presto verrai reindirizzato alla tua pagina del profilo.",
          error_title: "Qualcosa non ha funzionato.",
          error_message: "Riprova, per favore.",
        },
        errors: {
          name: {
            required: "Il nome è obbligatorio",
            max_length: "Il nome dello spazio di lavoro non deve superare gli 80 caratteri",
          },
          company_size: {
            required: "La dimensione aziendale è obbligatoria",
            select_a_range: "Seleziona la dimensione dell'organizzazione",
          },
        },
      },
      members: {
        title: "Membri",
        add_member: "Aggiungi membro",
        pending_invites: "Inviti in sospeso",
        invitations_sent_successfully: "Inviti inviati con successo",
        leave_confirmation:
          "Sei sicuro di voler lasciare lo spazio di lavoro? Non avrai più accesso a questo spazio. Questa azione non può essere annullata.",
        details: {
          full_name: "Nome completo",
          display_name: "Nome visualizzato",
          email_address: "Indirizzo email",
          account_type: "Tipo di account",
          authentication: "Autenticazione",
          joining_date: "Data di ingresso",
        },
        modal: {
          title: "Invita persone a collaborare",
          description: "Invita persone a collaborare nel tuo spazio di lavoro.",
          button: "Invia inviti",
          button_loading: "Invio inviti in corso",
          placeholder: "nome@azienda.com",
          errors: {
            required: "Abbiamo bisogno di un indirizzo email per invitarli.",
            invalid: "L'email non è valida",
          },
        },
      },
      billing_and_plans: {
        title: "Fatturazione e Piani",
        current_plan: "Piano attuale",
        free_plan: "Stai attualmente utilizzando il piano gratuito",
        view_plans: "Visualizza piani",
        heading: "Fatturazione e piani",
        description:
          "Scegli il tuo piano, gestisci gli abbonamenti e aggiorna facilmente man mano che le tue esigenze crescono.",
      },
      exports: {
        title: "Esportazioni",
        exporting: "Esportazione in corso",
        previous_exports: "Esportazioni precedenti",
        export_separate_files: "Esporta i dati in file separati",
        filters_info: "Applica filtri per esportare elementi di lavoro specifici in base ai tuoi criteri.",
        modal: {
          title: "Esporta in",
          toasts: {
            success: {
              title: "Esportazione riuscita",
              message: "Potrai scaricare gli {entity} esportati dall'esportazione precedente.",
            },
            error: {
              title: "Esportazione fallita",
              message: "L'esportazione non è riuscita. Per favore, riprova.",
            },
          },
        },
        heading: "Esportazioni",
        description:
          "Esporta i dati del tuo progetto in vari formati e accedi alla cronologia delle esportazioni con link per il download.",
        exporting_projects: "Esportazione del progetto",
        format: "Formato",
      },
      webhooks: {
        title: "Webhooks",
        add_webhook: "Aggiungi webhook",
        modal: {
          title: "Crea webhook",
          details: "Dettagli del webhook",
          payload: "URL del payload",
          question: "Quali eventi vuoi attivino questo webhook?",
          error: "L'URL è obbligatorio",
        },
        secret_key: {
          title: "Chiave segreta",
          message: "Genera un token per accedere al payload del webhook",
        },
        options: {
          all: "Inviami tutto",
          individual: "Seleziona eventi individuali",
        },
        toasts: {
          created: {
            title: "Webhook creato",
            message: "Il webhook è stato creato con successo",
          },
          not_created: {
            title: "Webhook non creato",
            message: "Il webhook non può essere creato",
          },
          updated: {
            title: "Webhook aggiornato",
            message: "Il webhook è stato aggiornato con successo",
          },
          not_updated: {
            title: "Webhook non aggiornato",
            message: "Il webhook non può essere aggiornato",
          },
          removed: {
            title: "Webhook rimosso",
            message: "Il webhook è stato rimosso con successo",
          },
          not_removed: {
            title: "Webhook non rimosso",
            message: "Il webhook non può essere rimosso",
          },
          secret_key_copied: {
            message: "Chiave segreta copiata negli appunti.",
          },
          secret_key_not_copied: {
            message: "Errore durante la copia della chiave segreta.",
          },
        },
        description: "Automatizza le notifiche verso servizi esterni quando si verificano eventi del progetto.",
      },
      api_tokens: {
        title: "Token API",
        add_token: "Aggiungi token API",
        create_token: "Crea token",
        never_expires: "Non scade mai",
        generate_token: "Genera token",
        generating: "Generazione in corso",
        delete: {
          title: "Elimina token API",
          description:
            "Qualsiasi applicazione che utilizza questo token non avrà più accesso ai dati di Pi Dash. Questa azione non può essere annullata.",
          success: {
            title: "Successo!",
            message: "Il token API è stato eliminato con successo",
          },
          error: {
            title: "Errore!",
            message: "Il token API non può essere eliminato",
          },
        },
      },
    },
    empty_state: {
      api_tokens: {
        title: "Nessun token API creato",
        description:
          "Le API di Pi Dash possono essere utilizzate per integrare i tuoi dati in Pi Dash con qualsiasi sistema esterno. Crea un token per iniziare.",
      },
      webhooks: {
        title: "Nessun webhook aggiunto",
        description: "Crea webhook per ricevere aggiornamenti in tempo reale e automatizzare azioni.",
      },
      exports: {
        title: "Nessuna esportazione ancora",
        description: "Ogni volta che esporti, avrai anche una copia qui per riferimento.",
      },
      imports: {
        title: "Nessuna importazione ancora",
        description: "Trova qui tutte le tue importazioni precedenti e scaricale.",
      },
    },
  },
  profile: {
    label: "Profilo",
    page_label: "Il tuo lavoro",
    work: "Lavoro",
    details: {
      joined_on: "Iscritto il",
      time_zone: "Fuso orario",
    },
    stats: {
      workload: "Carico di lavoro",
      overview: "Panoramica",
      created: "Elementi di lavoro creati",
      assigned: "Elementi di lavoro assegnati",
      subscribed: "Elementi di lavoro iscritti",
      state_distribution: {
        title: "Elementi di lavoro per stato",
        empty: "Crea elementi di lavoro per visualizzarli per stato nel grafico per un'analisi migliore.",
      },
      priority_distribution: {
        title: "Elementi di lavoro per priorità",
        empty: "Crea elementi di lavoro per visualizzarli per priorità nel grafico per un'analisi migliore.",
        priority: "",
      },
      recent_activity: {
        title: "Attività recente",
        empty: "Non abbiamo trovato dati. Per favore, controlla i tuoi input",
        button: "Scarica l'attività di oggi",
        button_loading: "Download in corso",
      },
    },
    actions: {
      profile: "Profilo",
      security: "Sicurezza",
      activity: "Attività",
      appearance: "Aspetto",
      notifications: "Notifiche",
    },
    tabs: {
      summary: "Riepilogo",
      assigned: "Assegnati",
      created: "Creati",
      subscribed: "Iscritti",
      activity: "Attività",
    },
    empty_state: {
      activity: {
        title: "Nessuna attività ancora",
        description:
          "Inizia creando un nuovo elemento di lavoro! Aggiungi dettagli e proprietà ad esso. Esplora Pi Dash per vedere la tua attività.",
      },
      assigned: {
        title: "Nessun elemento di lavoro assegnato a te",
        description: "Gli elementi di lavoro assegnati a te possono essere tracciati da qui.",
      },
      created: {
        title: "Nessun elemento di lavoro ancora",
        description: "Tutti gli elementi di lavoro creati da te appariranno qui. Tracciali direttamente da qui.",
      },
      subscribed: {
        title: "Nessun elemento di lavoro ancora",
        description: "Iscriviti agli elementi di lavoro che ti interessano, tracciali tutti qui.",
      },
    },
  },
  project_settings: {
    general: {
      enter_project_id: "Inserisci l'ID del progetto",
      please_select_a_timezone: "Seleziona un fuso orario",
      archive_project: {
        title: "Archivia progetto",
        description:
          "Archiviare un progetto lo rimuoverà dal menu di navigazione laterale, anche se potrai sempre accedervi dalla pagina dei progetti. Potrai ripristinare il progetto o eliminarlo quando vuoi.",
        button: "Archivia progetto",
      },
      delete_project: {
        title: "Elimina progetto",
        description:
          "Eliminando un progetto, tutti i dati e le risorse all'interno di esso verranno rimossi definitivamente e non potranno essere recuperati.",
        button: "Elimina il mio progetto",
      },
      toast: {
        success: "Progetto aggiornato con successo",
        error: "Impossibile aggiornare il progetto. Per favore, riprova.",
      },
    },
    members: {
      label: "Membri",
      project_lead: "Responsabile del progetto",
      default_assignee: "Assegnatario predefinito",
      guest_super_permissions: {
        title: "Concedi accesso in sola lettura a tutti gli elementi di lavoro per gli utenti ospiti:",
        sub_heading: "Questo permetterà agli ospiti di visualizzare tutti gli elementi di lavoro del progetto.",
      },
      invite_members: {
        title: "Invita membri",
        sub_heading: "Invita membri a lavorare sul tuo progetto.",
        select_co_worker: "Seleziona un collega",
      },
    },
    states: {
      describe_this_state_for_your_members: "Descrivi questo stato per i tuoi membri.",
      empty_state: {
        title: "Nessuno stato disponibile per il gruppo {groupKey}",
        description: "Crea un nuovo stato",
      },
      members_edit: {
        toast: {
          loading: "Aggiornamento impostazione del progetto...",
          success_title: "Operazione riuscita!",
          success_message: "Impostazione del progetto aggiornata.",
          error_title: "Errore!",
          error_message: "Si è verificato un errore durante l'aggiornamento dell'impostazione del progetto. Riprova.",
        },
        title: "Consenti ai membri di modificare gli stati",
        description:
          "Se abilitata, i membri del progetto possono aggiungere, modificare, riordinare ed eliminare gli stati del flusso di lavoro. Se disabilitata, solo gli amministratori possono gestire gli stati.",
      },
      heading: "Stati",
      description:
        "Definisci e personalizza gli stati del flusso di lavoro per monitorare l'avanzamento dei tuoi elementi di lavoro.",
    },
    labels: {
      label_title: "Titolo etichetta",
      label_title_is_required: "Il titolo dell'etichetta è obbligatorio",
      label_max_char: "Il nome dell'etichetta non deve superare i 255 caratteri",
      toast: {
        error: "Errore durante l'aggiornamento dell'etichetta",
      },
      heading: "Etichette",
      description: "Crea etichette personalizzate per categorizzare e organizzare i tuoi elementi di lavoro",
    },
    estimates: {
      label: "Stime",
      title: "Abilita le stime per il mio progetto",
      description: "Ti aiutano a comunicare la complessità e il carico di lavoro del team.",
      no_estimate: "Nessuna stima",
      new: "Nuovo sistema di stima",
      create: {
        custom: "Personalizzato",
        start_from_scratch: "Inizia da zero",
        choose_template: "Scegli un modello",
        choose_estimate_system: "Scegli un sistema di stima",
        enter_estimate_point: "Inserisci stima",
        step: "Passo {step} di {total}",
        label: "Crea stima",
      },
      toasts: {
        created: {
          success: {
            title: "Stima creata",
            message: "La stima è stata creata con successo",
          },
          error: {
            title: "Creazione stima fallita",
            message: "Non siamo riusciti a creare la nuova stima, riprova.",
          },
        },
        updated: {
          success: {
            title: "Stima modificata",
            message: "La stima è stata aggiornata nel tuo progetto.",
          },
          error: {
            title: "Modifica stima fallita",
            message: "Non siamo riusciti a modificare la stima, riprova",
          },
        },
        enabled: {
          success: {
            title: "Successo!",
            message: "Le stime sono state abilitate.",
          },
        },
        disabled: {
          success: {
            title: "Successo!",
            message: "Le stime sono state disabilitate.",
          },
          error: {
            title: "Errore!",
            message: "Impossibile disabilitare la stima. Riprova",
          },
        },
      },
      validation: {
        min_length: "La stima deve essere maggiore di 0.",
        unable_to_process: "Non possiamo elaborare la tua richiesta, riprova.",
        numeric: "La stima deve essere un valore numerico.",
        character: "La stima deve essere un valore di carattere.",
        empty: "Il valore della stima non può essere vuoto.",
        already_exists: "Il valore della stima esiste già.",
        unsaved_changes: "Hai delle modifiche non salvate. Salva prima di cliccare su Fatto",
        remove_empty:
          "La stima non può essere vuota. Inserisci un valore in ogni campo o rimuovi quelli per cui non hai valori.",
      },
      systems: {
        points: {
          label: "Punti",
          fibonacci: "Fibonacci",
          linear: "Lineare",
          squares: "Quadrati",
          custom: "Personalizzato",
        },
        categories: {
          label: "Categorie",
          t_shirt_sizes: "Taglie T-Shirt",
          easy_to_hard: "Da facile a difficile",
          custom: "Personalizzato",
        },
        time: {
          label: "Tempo",
          hours: "Ore",
        },
      },
      heading: "Stime",
      enable_description: "Ti aiutano a comunicare la complessità e il carico di lavoro del team.",
    },
    automations: {
      label: "Automatizzazioni",
      "auto-archive": {
        title: "Archivia automaticamente gli elementi di lavoro chiusi",
        description: "Pi Dash archiverà automaticamente gli elementi di lavoro che sono stati completati o annullati.",
        duration: "Archivia automaticamente gli elementi di lavoro chiusi per",
      },
      "auto-close": {
        title: "Chiudi automaticamente gli elementi di lavoro",
        description:
          "Pi Dash chiuderà automaticamente gli elementi di lavoro che non sono stati completati o annullati.",
        duration: "Chiudi automaticamente gli elementi di lavoro inattivi per",
        auto_close_status: "Stato di chiusura automatica",
      },
      heading: "Automazioni",
      description:
        "Configura azioni automatizzate per ottimizzare il flusso di lavoro di gestione del progetto e ridurre le attività manuali.",
    },
    empty_state: {
      labels: {
        title: "Nessuna etichetta ancora",
        description: "Crea etichette per aiutare a organizzare e filtrare gli elementi di lavoro nel tuo progetto.",
      },
      estimates: {
        title: "Nessun sistema di stime ancora",
        description: "Crea un set di stime per comunicare la quantità di lavoro per elemento di lavoro.",
        primary_button: "Aggiungi sistema di stime",
      },
    },
    features: {
      cycles: {
        title: "Cicli",
        short_title: "Cicli",
        description:
          "Pianifica il lavoro in periodi flessibili che si adattano al ritmo e al tempo unici di questo progetto.",
        toggle_title: "Abilita cicli",
        toggle_description: "Pianifica il lavoro in periodi di tempo mirati.",
      },
      modules: {
        title: "Moduli",
        short_title: "Moduli",
        description: "Organizza il lavoro in sotto-progetti con responsabili e assegnatari dedicati.",
        toggle_title: "Abilita moduli",
        toggle_description: "I membri del progetto potranno creare e modificare moduli.",
      },
      views: {
        title: "Viste",
        short_title: "Viste",
        description:
          "Salva ordinamenti, filtri e opzioni di visualizzazione personalizzati o condividili con il tuo team.",
        toggle_title: "Abilita viste",
        toggle_description: "I membri del progetto potranno creare e modificare viste.",
      },
      pages: {
        title: "Pagine",
        short_title: "Pagine",
        description: "Crea e modifica contenuti liberi: note, documenti, qualsiasi cosa.",
        toggle_title: "Abilita pagine",
        toggle_description: "I membri del progetto potranno creare e modificare pagine.",
      },
      intake: {
        title: "Ricezione",
        short_title: "Ricezione",
        description:
          "Consenti ai non membri di condividere bug, feedback e suggerimenti; senza interrompere il tuo flusso di lavoro.",
        toggle_title: "Abilita ricezione",
        toggle_description: "Consenti ai membri del progetto di creare richieste di ricezione nell'app.",
      },
    },
  },
  project_cycles: {
    add_cycle: "Aggiungi ciclo",
    more_details: "Altri dettagli",
    cycle: "Ciclo",
    update_cycle: "Aggiorna ciclo",
    create_cycle: "Crea ciclo",
    no_matching_cycles: "Nessun ciclo corrispondente",
    remove_filters_to_see_all_cycles: "Rimuovi i filtri per vedere tutti i cicli",
    remove_search_criteria_to_see_all_cycles: "Rimuovi i criteri di ricerca per vedere tutti i cicli",
    only_completed_cycles_can_be_archived: "Solo i cicli completati possono essere archiviati",
    start_date: "Data di inizio",
    end_date: "Data di fine",
    in_your_timezone: "Nel tuo fuso orario",
    transfer_work_items: "Trasferisci {count} elementi di lavoro",
    date_range: "Intervallo di date",
    add_date: "Aggiungi data",
    active_cycle: {
      label: "Ciclo attivo",
      progress: "Avanzamento",
      chart: "Grafico di burndown",
      priority_issue: "Elementi di lavoro ad alta priorità",
      assignees: "Assegnatari",
      issue_burndown: "Burndown degli elementi di lavoro",
      ideal: "Ideale",
      current: "Corrente",
      labels: "Etichette",
    },
    upcoming_cycle: {
      label: "Ciclo in arrivo",
    },
    completed_cycle: {
      label: "Ciclo completato",
    },
    status: {
      days_left: "Giorni rimanenti",
      completed: "Completato",
      yet_to_start: "Non ancora iniziato",
      in_progress: "In corso",
      draft: "Bozza",
    },
    action: {
      restore: {
        title: "Ripristina ciclo",
        success: {
          title: "Ciclo ripristinato",
          description: "Il ciclo è stato ripristinato.",
        },
        failed: {
          title: "Ripristino del ciclo fallito",
          description: "Il ciclo non può essere ripristinato. Per favore, riprova.",
        },
      },
      favorite: {
        loading: "Aggiunta del ciclo ai preferiti in corso",
        success: {
          description: "Ciclo aggiunto ai preferiti.",
          title: "Successo!",
        },
        failed: {
          description: "Impossibile aggiungere il ciclo ai preferiti. Per favore, riprova.",
          title: "Errore!",
        },
      },
      unfavorite: {
        loading: "Rimozione del ciclo dai preferiti in corso",
        success: {
          description: "Ciclo rimosso dai preferiti.",
          title: "Successo!",
        },
        failed: {
          description: "Impossibile rimuovere il ciclo dai preferiti. Per favore, riprova.",
          title: "Errore!",
        },
      },
      update: {
        loading: "Aggiornamento del ciclo in corso",
        success: {
          description: "Ciclo aggiornato con successo.",
          title: "Successo!",
        },
        failed: {
          description: "Errore durante l'aggiornamento del ciclo. Per favore, riprova.",
          title: "Errore!",
        },
        error: {
          already_exists:
            "Hai già un ciclo nelle date indicate, se vuoi creare una bozza di ciclo, puoi farlo rimuovendo entrambe le date.",
        },
      },
    },
    empty_state: {
      general: {
        title: "Raggruppa e definisci il tempo per il tuo lavoro in cicli.",
        description:
          "Suddividi il lavoro in blocchi temporali, lavora a ritroso dalla scadenza del tuo progetto per impostare le date e fai progressi tangibili come team.",
        primary_button: {
          text: "Imposta il tuo primo ciclo",
          comic: {
            title: "I cicli sono intervalli temporali ripetitivi.",
            description:
              "Uno sprint, un'iterazione o qualsiasi altro termine usato per il tracciamento settimanale o bisettimanale del lavoro è un ciclo.",
          },
        },
      },
      no_issues: {
        title: "Nessun elemento di lavoro aggiunto al ciclo",
        description: "Aggiungi o crea gli elementi di lavoro che desideri includere in questo ciclo",
        primary_button: {
          text: "Crea un nuovo elemento di lavoro",
        },
        secondary_button: {
          text: "Aggiungi un elemento di lavoro esistente",
        },
      },
      completed_no_issues: {
        title: "Nessun elemento di lavoro nel ciclo",
        description:
          "Nessun elemento di lavoro presente nel ciclo. Gli elementi di lavoro sono stati trasferiti o nascosti. Per visualizzare gli elementi nascosti, se presenti, aggiorna le proprietà di visualizzazione di conseguenza.",
      },
      active: {
        title: "Nessun ciclo attivo",
        description:
          "Un ciclo attivo è quello che include la data odierna nel suo intervallo. Visualizza qui i dettagli e l'avanzamento del ciclo attivo.",
      },
      archived: {
        title: "Nessun ciclo archiviato ancora",
        description:
          "Per organizzare il tuo progetto, archivia i cicli completati. Li troverai qui una volta archiviati.",
      },
    },
  },
  project_issues: {
    empty_state: {
      no_issues: {
        title: "Crea un elemento di lavoro e assegnalo a qualcuno, anche a te stesso",
        description:
          "Considera gli elementi di lavoro come compiti, attività, lavori o JTBD. Un elemento di lavoro e i suoi sotto-elementi di lavoro sono solitamente attività basate sul tempo assegnate ai membri del team. Il tuo team crea, assegna e completa gli elementi di lavoro per portare il progetto verso il suo obiettivo.",
        primary_button: {
          text: "Crea il tuo primo elemento di lavoro",
          comic: {
            title: "Gli elementi di lavoro sono i mattoni fondamentali in Pi Dash.",
            description:
              "Ridisegna l'interfaccia di Pi Dash, rebranding dell'azienda o lancia il nuovo sistema di iniezione del carburante sono esempi di elementi di lavoro che probabilmente hanno sotto-elementi.",
          },
        },
      },
      no_archived_issues: {
        title: "Nessun elemento di lavoro archiviato ancora",
        description:
          "Manualmente o tramite automazione, puoi archiviare gli elementi di lavoro che sono stati completati o annullati. Li troverai qui una volta archiviati.",
        primary_button: {
          text: "Imposta l'automazione",
        },
      },
      issues_empty_filter: {
        title: "Nessun elemento di lavoro trovato corrispondente ai filtri applicati",
        secondary_button: {
          text: "Cancella tutti i filtri",
        },
      },
    },
  },
  project_module: {
    add_module: "Aggiungi Modulo",
    update_module: "Aggiorna Modulo",
    create_module: "Crea Modulo",
    archive_module: "Archivia Modulo",
    restore_module: "Ripristina Modulo",
    delete_module: "Elimina modulo",
    empty_state: {
      general: {
        title: "Associa i traguardi del tuo progetto ai Moduli e traccia facilmente il lavoro aggregato.",
        description:
          "Un gruppo di elementi di lavoro che appartengono a un genitore logico e gerarchico forma un modulo. Considerali come un modo per tracciare il lavoro in base ai traguardi del progetto. Hanno i propri intervalli temporali e scadenze, oltre ad analisi che ti aiutano a vedere quanto sei vicino o lontano da un traguardo.",
        primary_button: {
          text: "Crea il tuo primo modulo",
          comic: {
            title: "I moduli aiutano a raggruppare il lavoro per gerarchia.",
            description:
              "Un modulo per il carrello, un modulo per il telaio e un modulo per il magazzino sono tutti buoni esempi di questo raggruppamento.",
          },
        },
      },
      no_issues: {
        title: "Nessun elemento di lavoro nel modulo",
        description: "Crea o aggiungi elementi di lavoro che desideri completare come parte di questo modulo",
        primary_button: {
          text: "Crea nuovi elementi di lavoro",
        },
        secondary_button: {
          text: "Aggiungi un elemento di lavoro esistente",
        },
      },
      archived: {
        title: "Nessun modulo archiviato ancora",
        description:
          "Per organizzare il tuo progetto, archivia i moduli completati o annullati. Li troverai qui una volta archiviati.",
      },
      sidebar: {
        in_active: "Questo modulo non è ancora attivo.",
        invalid_date: "Data non valida. Inserisci una data valida.",
      },
    },
    quick_actions: {
      archive_module: "Archivia modulo",
      archive_module_description: "Solo i moduli completati o annullati possono essere archiviati.",
      delete_module: "Elimina modulo",
    },
    toast: {
      copy: {
        success: "Link del modulo copiato negli appunti",
      },
      delete: {
        success: "Modulo eliminato con successo",
        error: "Impossibile eliminare il modulo",
      },
    },
  },
  project_views: {
    empty_state: {
      general: {
        title: "Salva visualizzazioni filtrate per il tuo progetto. Crea quante ne vuoi",
        description:
          "Le visualizzazioni sono un insieme di filtri salvati che usi frequentemente o a cui vuoi avere accesso rapido. Tutti i tuoi colleghi in un progetto possono vedere tutte le visualizzazioni e scegliere quella che fa per loro.",
        primary_button: {
          text: "Crea la tua prima visualizzazione",
          comic: {
            title: "Le visualizzazioni si basano sulle proprietà degli elementi di lavoro.",
            description: "Puoi creare una visualizzazione da qui con quante proprietà e filtri desideri.",
          },
        },
      },
      filter: {
        title: "Nessuna visualizzazione corrispondente",
        description:
          "Nessuna visualizzazione corrisponde ai criteri di ricerca. \n Crea una nuova visualizzazione invece.",
      },
    },
    delete_view: {
      title: "Sei sicuro di voler eliminare questa visualizzazione?",
      content:
        "Se confermi, tutte le opzioni di ordinamento, filtro e visualizzazione + il layout che hai scelto per questa visualizzazione saranno eliminate permanentemente senza possibilità di ripristinarle.",
    },
  },
  project_page: {
    empty_state: {
      general: {
        title:
          "Scrivi una nota, un documento o una vera e propria base di conoscenza. Fai partire Galileo, l'assistente AI di Pi Dash, per aiutarti a iniziare",
        description:
          "Le pagine sono spazi per appunti in Pi Dash. Prendi note durante le riunioni, formattale facilmente, inserisci elementi di lavoro, disponili usando una libreria di componenti e tienili tutti nel contesto del tuo progetto. Per velocizzare qualsiasi documento, invoca Galileo, l'IA di Pi Dash, con una scorciatoia o con il clic di un pulsante.",
        primary_button: {
          text: "Crea la tua prima pagina",
        },
      },
      private: {
        title: "Nessuna pagina privata ancora",
        description:
          "Tieni qui i tuoi appunti privati. Quando sarai pronto a condividerli, il team sarà a portata di clic.",
        primary_button: {
          text: "Crea la tua prima pagina",
        },
      },
      public: {
        title: "Nessuna pagina pubblica ancora",
        description: "Visualizza qui le pagine condivise con tutti nel tuo progetto.",
        primary_button: {
          text: "Crea la tua prima pagina",
        },
      },
      archived: {
        title: "Nessuna pagina archiviata ancora",
        description: "Archivia le pagine che non sono più di tuo interesse. Potrai accedervi quando necessario.",
      },
    },
  },
  command_k: {
    empty_state: {
      search: {
        title: "Nessun risultato trovato",
      },
    },
  },
  issue_relation: {
    empty_state: {
      search: {
        title: "Nessun elemento di lavoro corrispondente trovato",
      },
      no_issues: {
        title: "Nessun elemento di lavoro trovato",
      },
    },
  },
  issue_comment: {
    empty_state: {
      general: {
        title: "Nessun commento ancora",
        description: "I commenti possono essere usati come spazio per discussioni e follow-up sugli elementi di lavoro",
      },
    },
  },
  notification: {
    label: "Notifiche",
    page_label: "{workspace} - Notifiche",
    options: {
      mark_all_as_read: "Segna tutto come letto",
      mark_read: "Segna come letto",
      mark_unread: "Segna come non letto",
      refresh: "Aggiorna",
      filters: "Filtri Notifiche",
      show_unread: "Mostra non lette",
      show_snoozed: "Mostra snoozate",
      show_archived: "Mostra archiviate",
      mark_archive: "Archivia",
      mark_unarchive: "Rimuovi da archivio",
      mark_snooze: "Snoozed",
      mark_unsnooze: "Annulla snooze",
    },
    toasts: {
      read: "Notifica segnata come letta",
      unread: "Notifica segnata come non letta",
      archived: "Notifica archiviata",
      unarchived: "Notifica rimossa dall'archivio",
      snoozed: "Notifica snoozata",
      unsnoozed: "Notifica desnoozata",
      un_snoozed: "",
    },
    empty_state: {
      detail: {
        title: "Seleziona per visualizzare i dettagli.",
      },
      all: {
        title: "Nessun elemento di lavoro assegnato",
        description: "Qui puoi vedere gli aggiornamenti degli elementi di lavoro assegnati a te",
      },
      mentions: {
        title: "Nessun elemento di lavoro assegnato",
        description: "Qui puoi vedere gli aggiornamenti degli elementi di lavoro assegnati a te",
      },
    },
    tabs: {
      all: "Tutti",
      mentions: "Menzioni",
    },
    filter: {
      assigned: "Assegnati a me",
      created: "Creati da me",
      subscribed: "Iscritti da me",
    },
    snooze: {
      "1_day": "1 giorno",
      "3_days": "3 giorni",
      "5_days": "5 giorni",
      "1_week": "1 settimana",
      "2_weeks": "2 settimane",
      custom: "Personalizzato",
    },
  },
  active_cycle: {
    empty_state: {
      progress: {
        title: "Aggiungi elementi di lavoro al ciclo per visualizzarne l'avanzamento",
      },
      chart: {
        title: "Aggiungi elementi di lavoro al ciclo per visualizzare il grafico di burndown.",
      },
      priority_issue: {
        title: "Visualizza in anteprima gli elementi di lavoro ad alta priorità del ciclo.",
      },
      assignee: {
        title: "Aggiungi assegnatari agli elementi di lavoro per vedere la ripartizione per assegnatario.",
      },
      label: {
        title: "Aggiungi etichette agli elementi di lavoro per vedere la ripartizione per etichette.",
      },
    },
  },
  disabled_project: {
    empty_state: {
      inbox: {
        title: "L'accoglienza non è abilitata per il progetto.",
        description:
          "L'accoglienza ti aiuta a gestire le richieste in entrata per il tuo progetto e ad aggiungerle come elementi di lavoro nel tuo flusso. Abilita l'accoglienza dalle impostazioni del progetto per gestire le richieste.",
        primary_button: {
          text: "Gestisci funzionalità",
        },
      },
      cycle: {
        title: "I cicli non sono abilitati per questo progetto.",
        description:
          "Suddividi il lavoro in blocchi temporali, lavora a ritroso dalla scadenza del tuo progetto per impostare le date e fai progressi tangibili come team. Abilita la funzionalità dei cicli per il tuo progetto per iniziare a usarli.",
        primary_button: {
          text: "Gestisci funzionalità",
        },
      },
      module: {
        title: "I moduli non sono abilitati per il progetto.",
        description:
          "I moduli sono i blocchi costitutivi del tuo progetto. Abilita i moduli dalle impostazioni del progetto per iniziare a usarli.",
        primary_button: {
          text: "Gestisci funzionalità",
        },
      },
      page: {
        title: "Le pagine non sono abilitate per il progetto.",
        description:
          "Le pagine sono i blocchi costitutivi del tuo progetto. Abilita le pagine dalle impostazioni del progetto per iniziare a usarle.",
        primary_button: {
          text: "Gestisci funzionalità",
        },
      },
      view: {
        title: "Le visualizzazioni non sono abilitate per il progetto.",
        description:
          "Le visualizzazioni sono i blocchi costitutivi del tuo progetto. Abilita le visualizzazioni dalle impostazioni del progetto per iniziare a usarle.",
        primary_button: {
          text: "Gestisci funzionalità",
        },
      },
    },
  },
  workspace_draft_issues: {
    draft_an_issue: "Bozza di un elemento di lavoro",
    empty_state: {
      title: "Le bozze degli elementi di lavoro e, presto, anche i commenti appariranno qui.",
      description:
        "Per provarlo, inizia ad aggiungere un elemento di lavoro e lascialo a metà o crea la tua prima bozza qui sotto. 😉",
      primary_button: {
        text: "Crea la tua prima bozza",
      },
    },
    delete_modal: {
      title: "Elimina bozza",
      description: "Sei sicuro di voler eliminare questa bozza? Questa azione non può essere annullata.",
    },
    toasts: {
      created: {
        success: "Bozza creata",
        error: "Impossibile creare l'elemento di lavoro. Per favore, riprova.",
      },
      deleted: {
        success: "Bozza eliminata",
      },
    },
  },
  stickies: {
    title: "I tuoi stickies",
    placeholder: "clicca per scrivere qui",
    all: "Tutti gli stickies",
    "no-data": "Annota un'idea, cattura un aha o registra un lampo di genio. Aggiungi uno sticky per iniziare.",
    add: "Aggiungi sticky",
    search_placeholder: "Cerca per titolo",
    delete: "Elimina sticky",
    delete_confirmation: "Sei sicuro di voler eliminare questo sticky?",
    empty_state: {
      simple: "Annota un'idea, cattura un aha o registra un lampo di genio. Aggiungi uno sticky per iniziare.",
      general: {
        title: "Gli stickies sono note rapide e cose da fare che annoti al volo.",
        description:
          "Cattura i tuoi pensieri e idee senza sforzo creando stickies a cui puoi accedere in qualsiasi momento e ovunque.",
        primary_button: {
          text: "Aggiungi sticky",
        },
      },
      search: {
        title: "Non corrisponde a nessuno dei tuoi stickies.",
        description: "Prova con un termine diverso o facci sapere se sei sicuro che la tua ricerca sia corretta.",
        primary_button: {
          text: "Aggiungi sticky",
        },
      },
    },
    toasts: {
      errors: {
        wrong_name: "Il nome dello sticky non può superare i 100 caratteri.",
        already_exists: "Esiste già uno sticky senza descrizione",
      },
      created: {
        title: "Sticky creato",
        message: "Lo sticky è stato creato con successo",
      },
      not_created: {
        title: "Sticky non creato",
        message: "Lo sticky non può essere creato",
      },
      updated: {
        title: "Sticky aggiornato",
        message: "Lo sticky è stato aggiornato con successo",
      },
      not_updated: {
        title: "Sticky non aggiornato",
        message: "Lo sticky non può essere aggiornato",
      },
      removed: {
        title: "Sticky rimosso",
        message: "Lo sticky è stato rimosso con successo",
      },
      not_removed: {
        title: "Sticky non rimosso",
        message: "Lo sticky non può essere rimosso",
      },
    },
  },
  role_details: {
    guest: {
      title: "Ospite",
      description: "I membri esterni alle organizzazioni possono essere invitati come ospiti.",
    },
    member: {
      title: "Membro",
      description:
        "Permette di leggere, scrivere, modificare ed eliminare entità all'interno di progetti, cicli e moduli.",
    },
    admin: {
      title: "Amministratore",
      description: "Tutti i permessi impostati su true all'interno dello spazio di lavoro.",
    },
  },
  user_roles: {
    product_or_project_manager: "Product / Project Manager",
    development_or_engineering: "Sviluppo / Ingegneria",
    founder_or_executive: "Fondatore / Dirigente",
    freelancer_or_consultant: "Freelance / Consulente",
    marketing_or_growth: "Marketing / Crescita",
    sales_or_business_development: "Vendite / Sviluppo commerciale",
    support_or_operations: "Supporto / Operazioni",
    student_or_professor: "Studente / Professore",
    human_resources: "Risorse umane",
    other: "Altro",
  },
  importer: {
    github: {
      title: "Github",
      description: "Importa elementi di lavoro dai repository GitHub e sincronizzali.",
    },
    jira: {
      title: "Jira",
      description: "Importa elementi di lavoro ed epic dai progetti e dagli epic di Jira.",
    },
  },
  exporter: {
    csv: {
      title: "CSV",
      description: "Esporta elementi di lavoro in un file CSV.",
      short_description: "Esporta come CSV",
    },
    excel: {
      title: "Excel",
      description: "Esporta elementi di lavoro in un file Excel.",
      short_description: "Esporta come Excel",
    },
    xlsx: {
      title: "Excel",
      description: "Esporta elementi di lavoro in un file Excel.",
      short_description: "Esporta come Excel",
    },
    json: {
      title: "JSON",
      description: "Esporta elementi di lavoro in un file JSON.",
      short_description: "Esporta come JSON",
    },
  },
  default_global_view: {
    all_issues: "Tutti gli elementi di lavoro",
    assigned: "Assegnati",
    created: "Creati",
    subscribed: "Iscritti",
  },
  themes: {
    theme_options: {
      system_preference: {
        label: "Preferenza di sistema",
      },
      light: {
        label: "Chiaro",
      },
      dark: {
        label: "Scuro",
      },
      light_contrast: {
        label: "Contrasto elevato chiaro",
      },
      dark_contrast: {
        label: "Contrasto elevato scuro",
      },
      custom: {
        label: "Tema personalizzato",
      },
    },
  },
  project_modules: {
    status: {
      backlog: "Backlog",
      planned: "Pianificato",
      in_progress: "In corso",
      paused: "In pausa",
      completed: "Completato",
      cancelled: "Annullato",
    },
    layout: {
      list: "Layout a lista",
      board: "Layout a galleria",
      timeline: "Layout a timeline",
    },
    order_by: {
      name: "Nome",
      progress: "Avanzamento",
      issues: "Numero di elementi di lavoro",
      due_date: "Scadenza",
      created_at: "Data di creazione",
      manual: "Manuale",
    },
  },
  cycle: {
    label: "{count, plural, one {Ciclo} other {Cicli}}",
    no_cycle: "Nessun ciclo",
  },
  module: {
    label: "{count, plural, one {Modulo} other {Moduli}}",
    no_module: "Nessun modulo",
  },
  description_versions: {
    last_edited_by: "Ultima modifica di",
    previously_edited_by: "Precedentemente modificato da",
    edited_by: "Modificato da",
  },
  self_hosted_maintenance_message: {
    pi_dash_didnt_start_up_this_could_be_because_one_or_more_pi_dash_services_failed_to_start:
      "Pi Dash non si è avviato. Questo potrebbe essere dovuto al fatto che uno o più servizi Pi Dash non sono riusciti ad avviarsi.",
    choose_view_logs_from_setup_sh_and_docker_logs_to_be_sure:
      "Scegli View Logs da setup.sh e dai log Docker per essere sicuro.",
  },
  page_navigation_pane: {
    tabs: {
      outline: {
        label: "Schema",
        empty_state: {
          title: "Intestazioni mancanti",
          description: "Aggiungiamo alcune intestazioni a questa pagina per vederle qui.",
        },
      },
      info: {
        label: "Info",
        document_info: {
          words: "Parole",
          characters: "Caratteri",
          paragraphs: "Paragrafi",
          read_time: "Tempo di lettura",
        },
        actors_info: {
          edited_by: "Modificato da",
          created_by: "Creato da",
        },
        version_history: {
          label: "Cronologia versioni",
          current_version: "Versione corrente",
        },
      },
      assets: {
        label: "Risorse",
        download_button: "Scarica",
        empty_state: {
          title: "Immagini mancanti",
          description: "Aggiungi immagini per vederle qui.",
        },
      },
    },
    open_button: "Apri pannello di navigazione",
    close_button: "Chiudi pannello di navigazione",
    outline_floating_button: "Apri schema",
  },
  run_ai: {
    run_button: "Run AI",
    run_button_tooltip:
      "Manually trigger an extra AI agent run. (Issues in the In Progress state already tick an agent run every few hours.)",
    comment_button: "Comment & Run",
    modal_title: "Comment & Run",
    modal_description: "Post a comment on this work item and start an AI agent run with the comment as the prompt.",
    placeholder: "Tell the agent what you want it to do...",
    posting: "Posting...",
    starting: "Starting run...",
    success_title: "Agent run started",
    success_message: "The AI agent will pick up this work item shortly.",
    failed_title: "Failed to start agent run",
    failed_workspace_title: "Could not start agent run",
    failed_message: "Could not start the agent run. Please try again.",
    workspace_not_found: "Workspace not found.",
    comment_failed_title: "Could not post comment",
    comment_failed_message: "Failed to post the comment.",
  },
  scheduler_bindings: {
    tab_label: "Pianificatori",
    title: "Pianificatori",
    subtitle:
      "Pianificatori installati su questo progetto. Ogni installazione attiva il suo prompt sul progetto secondo il cron configurato.",
    install: "Installa pianificatore",
    columns: {
      name: "Pianificatore",
      cron: "Programmazione",
      next_run: "Prossima esecuzione",
      last_run: "Ultima esecuzione",
      status: "Stato",
      updated: "Aggiornato",
    },
    toast: {
      updated_title: "Installazione aggiornata",
      enabled_message: "Pianificatore abilitato — verrà attivato al prossimo tick del cron.",
      disabled_message: "Pianificatore disabilitato — non verrà attivato finché non verrà riabilitato.",
      error_title: "Qualcosa è andato storto",
      update_failed: "Impossibile aggiornare l'installazione.",
      updated_message: "Le esecuzioni successive utilizzeranno le nuove impostazioni.",
      installed_title: "Pianificatore installato",
      installed_message: "Verrà attivato secondo il cron configurato.",
      install_failed: "Impossibile installare lo scheduler.",
      uninstalled_title: "Scheduler disinstallato",
      uninstalled_message: "Non verrà attivato su questo progetto finché non verrà reinstallato.",
      uninstall_failed: "Impossibile disinstallare lo scheduler.",
    },
    list: {
      empty:
        "Nessuno scheduler installato su questo progetto. Fai clic su “Installa scheduler” per aggiungerne uno dal catalogo dell'area di lavoro.",
      none_yet: "(mai)",
    },
    actions: {
      disable: "Disabilita scheduler",
      enable: "Abilita scheduler",
      edit: "Modifica",
      uninstall: "Disinstalla",
    },
    status: {
      enabled: "Abilitato",
      disabled: "Disabilitato",
    },
    edit_modal: {
      title: "Modifica installazione scheduler",
      saving: "Salvataggio in corso…",
      save: "Salva",
    },
    install_modal: {
      cron_label: "Programmazione (cron)",
      errors: {
        cron_required: "L'espressione cron è obbligatoria.",
        scheduler_required: "Scegli uno scheduler.",
      },
      cron_placeholder: "0 9 * * *",
      cron_help: "Espressione cron a 5 campi in UTC, ad es. ``0 9 * * *`` per le 09:00 UTC ogni giorno.",
      extra_context_label: "Contesto del progetto (opzionale)",
      extra_context_placeholder: "Note specifiche per questo progetto…",
      extra_context_help:
        "Aggiunto al prompt di base dello scheduler in fase di esecuzione. Usalo per fornire un contesto specifico del progetto che il prompt dell'area di lavoro non dovrebbe contenere.",
      enabled_label: "Abilitato",
      enabled_help: "Le installazioni disabilitate non vengono attivate sul cron finché non vengono riabilitate.",
      cancel: "Annulla",
      none_available_title: "Nessuno scheduler disponibile",
      none_available_body:
        "O tutti gli scheduler dell'area di lavoro sono già installati su questo progetto, oppure l'amministratore dell'area di lavoro non ne ha abilitati. Visita Area di lavoro → Scheduler per gestire il catalogo.",
      title: "Installa scheduler",
      scheduler_label: "Scheduler",
      scheduler_help: "Scegli tra gli scheduler abilitati del tuo workspace. Quelli già installati non sono elencati.",
      installing: "Installazione in corso…",
      install: "Installa",
    },
    uninstall_modal: {
      title: "Disinstallare lo scheduler?",
      body: "Lo scheduler smette di attivarsi su questo progetto. La definizione del workspace rimane invariata e può essere reinstallata in seguito.",
      confirm: "Disinstalla",
    },
  },
  prompts: {
    detail: {
      loading: "Caricamento in corso…",
      not_found: "Modello non trovato.",
      default_title: "Modello di prompt (predefinito Pi Dash)",
      workspace_title: "Modello di prompt (override del workspace)",
      default_description:
        "Questo è il predefinito integrato di Pi Dash. Gli amministratori del workspace non possono modificarlo qui — personalizza per il tuo workspace per sovrascriverlo.",
      workspace_description:
        "L'override del tuo workspace del predefinito di Pi Dash. Le modifiche incrementano la versione e si applicano alla prossima esecuzione dell'agente per questo workspace.",
      back: "Torna all'elenco",
      body: "Corpo del modello (Jinja + Markdown)",
      unsaved: "Modifiche non salvate",
      save: "Salva",
    },
    toast: {
      saved_title: "Prompt salvato",
      saved_message: "Le successive esecuzioni dell'agente utilizzeranno il prompt aggiornato.",
      save_failed: "Impossibile salvare il prompt.",
      error_title: "Qualcosa è andato storto",
      created_title: "Override del workspace creato",
      created_message: "Abbiamo copiato il predefinito corrente di Pi Dash. Modifica e salva per personalizzarlo.",
      customize_failed: "Impossibile creare l'override del workspace.",
      reverted_title: "Ripristinato al predefinito di Pi Dash",
      reverted_message: "Questo workspace è tornato al modello predefinito condiviso.",
      revert_failed: "Impossibile ripristinare il prompt.",
    },
    preview: {
      missing_issue_id: "Inserisci prima un ID issue.",
      failed: "Rendering fallito.",
      title: "Anteprima",
      issue_id_placeholder: "ID issue (UUID)",
      run: "Anteprima",
      empty: "Incolla un ID issue e fai clic su Anteprima per visualizzare il template su un issue reale.",
      admin_only:
        "Visualizzare l'anteprima del prompt renderizzato è un'azione riservata agli amministratori del workspace. Chiedi al tuo amministratore del workspace se hai bisogno di vederlo renderizzato su un issue specifico.",
    },
    scope: {
      default: "Predefinito Pi Dash",
      workspace: "Override del workspace",
    },
    title: "Prompt",
    subtitle:
      "Template di prompt di sistema che vengono renderizzati su ogni issue prima di un'esecuzione dell'agente. Gli amministratori del workspace possono personalizzare il predefinito per questo workspace.",
    customize: "Personalizza per questo workspace",
    columns: {
      name: "Nome",
      scope: "Ambito",
      version: "Versione",
      updated: "Aggiornato",
    },
    list: {
      empty: "Nessun template di prompt disponibile. Il predefinito Pi Dash verrà inserito alla prossima migrazione.",
    },
    revert: {
      confirm_title: "Ripristinare il predefinito Pi Dash?",
      confirm_body:
        "Questa operazione archivia il template con ambito workspace. Le nuove esecuzioni dell'agente in questo workspace utilizzeranno il predefinito Pi Dash finché non creerai un altro override.",
      confirm: "Ripristina",
    },
    actions: {
      edit: "Modifica",
      view: "Visualizza",
      revert: "Ripristina al predefinito",
    },
  },
  runners: {
    toast: {
      error_title: "Errore!",
    },
    approvals: {
      decision_failed: "Impossibile registrare la decisione",
      empty: "Nessuna approvazione in sospeso.",
      run_meta: "Esecuzione {runId} · richiesta {at}",
      expires: "scade {at}",
      accept_once: "Accetta una volta",
      accept_for_session: "Accetta per la sessione",
      decline: "Rifiuta",
    },
    tabs: {
      runners: "Agenti AI",
      runs: "Esecuzioni",
      approvals: "Approvazioni",
    },
    title: "Agenti AI",
    page_title: "{workspace} - Agenti AI",
    list: {
      delete_failed: "Impossibile eliminare il runner",
      revoke_failed: "Impossibile revocare il runner",
      revive_failed: "Impossibile riattivare il runner",
      add_runner: "Aggiungi runner",
      how_it_works_title: "Come aggiungere un runner",
      how_it_works_body:
        "1. Clicca su \"Aggiungi runner\", scegli un progetto + pod e invia. Il cloud genera un token di registrazione monouso legato a quel runner.\n2. Sulla macchina che ospiterà il runner, esegui il comando visualizzato `pidash connect --url ... --token ... --host-label ...`.\n3. Il demone si registra e il runner appare online qui.\n\nOgni runner ha il proprio token. Il primo runner registrato su un host avvia anche un token macchina utilizzato dalla CLI `pidash` per comandi non runner.\n\nPrerequisito: la CLI dell'agente (codex / claude) deve essere già installata sull'host.",
      connected_runners: "Runner",
      columns: {
        name: "Nome",
        status: "Stato",
        os_arch: "SO / Arch",
        version: "Versione",
        last_heartbeat: "Ultimo heartbeat",
      },
      columns_pod: "Pod",
      revive: "Riattiva",
      revoke: "Revoca",
      delete: "Elimina",
      empty:
        'Nessun runner ancora. Clicca su "Aggiungi runner" per generare il tuo primo token di registrazione per runner.',
      delete_confirm_title: "Eliminare il runner?",
      delete_confirm_body:
        "La riga del runner viene rimossa e il demone viene forzato offline. Le esecuzioni storiche vengono conservate con un riferimento runner nullo.",
      revoke_confirm_title: "Revocare il runner?",
      revoke_confirm_body:
        "Le credenziali del runner vengono invalidate e tutte le esecuzioni in corso vengono annullate, ma la riga rimane nell'elenco. Puoi riattivarla in seguito per generare un nuovo token di registrazione sulla stessa riga.",
      revive_modal_title: "Nuovo token di registrazione",
      revive_modal_body:
        "Esegui il comando seguente sull'host che dovrebbe riprendere questo runner. Copialo ora: il token non verrà più mostrato.",
      project_placeholder: "Seleziona un progetto",
      copy_failed: "Impossibile copiare negli appunti",
    },
    machine_token_note: {
      body: "La prima volta che un runner si registra su un nuovo host (cioè un nuovo ``host_label``), il cloud emette anche un token macchina utilizzato dalla CLI ``pidash`` per comandi non runner (issue, comment, state). I runner successivi sullo stesso host riutilizzano quel token.",
    },
    pods: {
      title: "Pod",
      help: "I Pod raggruppano i tuoi runner. Le issue delegano a un pod e qualsiasi runner libero al suo interno prende in carico il lavoro. Clicca su un riquadro per filtrare i runner.",
      load_failed: "Impossibile caricare i pod",
      tile_aria: "Filtra i runner per pod {name}",
      default_badge: "predefinito",
      runner_count: "{count} runner",
      create_tile: "Crea nuovo pod",
      filter_active: "Filtraggio dei runner per pod {name}",
      filter_clear: "Cancella filtro",
    },
    add_modal: {
      runner_id_label: "ID runner",
      done: "Fatto",
      agent_options: {
        claude_code: "Claude Code",
        codex: "Codex",
      },
      errors: {
        create_failed: "Impossibile creare il token di registrazione.",
        project_required: "Seleziona un progetto.",
        load_projects_failed: "Impossibile caricare i progetti.",
        load_pods_failed: "Impossibile caricare i pod.",
      },
      title: "Aggiungi runner",
      subtitle:
        "Crea un token di registrazione monouso per un nuovo runner. Eseguirai il comando `pidash connect` visualizzato sulla macchina che lo ospiterà.",
      project_label: "Progetto",
      project_help: "Il progetto su cui lavorerà questo runner.",
      pod_label: "Pod (opzionale)",
      pod_default_option: "(pod predefinito)",
      pod_help: "Per impostazione predefinita, utilizza il pod predefinito del progetto.",
      name_label: "Nome (opzionale)",
      name_placeholder: "my-laptop-runner",
      name_help: "Assegnato automaticamente se vuoto, ad es. ``runner_001``.",
      host_label_label: "Etichetta host (opzionale)",
      host_label_placeholder: "my-laptop",
      host_label_help:
        "Nome host libero incorporato nel comando suggerito. Il demone sostituirà il suo nome host effettivo se si lascia il flag vuoto.",
      working_dir_label: "Directory di lavoro (opzionale)",
      working_dir_placeholder: "directory di lavoro del progetto sulla macchina di sviluppo locale",
      working_dir_help:
        "Percorso locale in cui il demone esegue la CLI dell'agente — di solito il repository del progetto su disco. Per impostazione predefinita, viene utilizzata una sandbox nella directory dati del runner, cosa che raramente è quella desiderata.",
      agent_label: "Agente",
      agent_help:
        "Quale CLI dell'agente AI questo runner guiderà. Incorporato nel comando ``pidash connect`` visualizzato.",
      cancel: "Annulla",
      submitting: "Generazione in corso…",
      submit: "Genera token di iscrizione",
      token_warning: "Copia questo una volta — il token di iscrizione non verrà più mostrato.",
      token_instructions: "Esegui questo sul computer che ospiterà il runner:",
      copied: "Copiato!",
      copy_command: "Copia comando",
    },
    runs: {
      cancel_failed: "Impossibile annullare l'esecuzione",
      columns: {
        started: "Avviato",
        status: "Stato",
        prompt: "Prompt",
      },
      empty: "Nessuna esecuzione ancora.",
      select_run: "Seleziona un'esecuzione a sinistra.",
      cancel: "Annulla esecuzione",
      prompt: "Prompt",
      error: "Errore",
      done_payload: "Payload completato",
      events_count: "Eventi ({count})",
      event_columns: {
        seq: "seq",
        kind: "tipo",
        at: "alle",
      },
      cancel_confirm_title: "Annullare l'esecuzione?",
      cancel_confirm_body: "Il runner interromperà questa esecuzione non appena riceverà il segnale.",
    },
    create_pod_modal: {
      errors: {
        create_failed: "Impossibile creare il pod.",
        project_required: "Seleziona un progetto.",
        load_projects_failed: "Impossibile caricare i progetti.",
        name_required: "Il nome è obbligatorio.",
      },
      title: "Crea nuovo pod",
      subtitle: "I pod raggruppano i runner in un progetto. Scegli un progetto, poi dai un nome al pod.",
      project_label: "Progetto",
      project_placeholder: "Seleziona un progetto",
      project_help: "Il progetto a cui appartiene questo pod. Il nome sarà preceduto dall'identificatore del progetto.",
      name_label: "Nome",
      name_placeholder: "beefy",
      name_help: "Lettere, cifre, trattini e trattini bassi. Il prefisso del progetto viene aggiunto automaticamente.",
      description_label: "Descrizione (opzionale)",
      description_placeholder: "Dove viene eseguito questo pod, a cosa serve, ecc.",
      cancel: "Annulla",
      submitting: "Creazione in corso…",
      submit: "Crea pod",
    },
  },
  schedulers: {
    toast: {
      created_title: "Scheduler creato",
      created_message: "Gli amministratori del progetto possono ora installarlo sui loro progetti.",
      create_failed: "Impossibile creare lo scheduler.",
      error_title: "Qualcosa è andato storto",
      updated_title: "Scheduler aggiornato",
      updated_message: "Le esecuzioni successive utilizzeranno la definizione aggiornata.",
      update_failed: "Impossibile aggiornare lo scheduler.",
      deleted_title: "Scheduler eliminato",
      deleted_message: "I binding attivi hanno smesso di attivarsi.",
      delete_failed: "Impossibile eliminare lo scheduler.",
    },
    title: "Scheduler",
    subtitle:
      "Definizioni di scheduler riutilizzabili per questo workspace. Installane uno su un progetto per eseguire il suo prompt sul progetto con un cron.",
    new: "Nuovo scheduler",
    columns: {
      name: "Nome",
      slug: "Slug",
      source: "Origine",
      installs: "Installazioni",
      status: "Stato",
      updated: "Aggiornato",
    },
    list: {
      empty: "Ancora nessun scheduler in questo workspace. Fai clic su “Nuovo scheduler” per crearne uno.",
      installs_count: "{count, plural, one {# installazione} other {# installazioni}}",
    },
    source: {
      manifest: "Manifesto",
      builtin: "Integrato",
    },
    status: {
      enabled: "Abilitato",
      disabled: "Disabilitato",
    },
    actions: {
      edit: "Modifica",
      delete: "Elimina",
    },
    delete: {
      confirm_title: "Eliminare lo scheduler?",
      confirm_body:
        "Questa operazione elimina lo scheduler in modo soft. Tutti i binding attivi del progetto smetteranno di funzionare. Lo slug diventa disponibile per la ricreazione.",
      confirm: "Elimina",
    },
    form: {
      edit_title: "Modifica scheduler",
      create_title: "Nuovo scheduler",
      slug_label: "Slug",
      errors: {
        slug_required: "Lo slug è obbligatorio.",
        name_required: "Il nome è obbligatorio.",
        prompt_required: "Il prompt è obbligatorio.",
      },
      slug_placeholder: "security-audit",
      slug_help: "Identificatore in minuscolo utilizzato negli URL. Non può essere modificato dopo la creazione.",
      name_label: "Nome",
      name_placeholder: "Security audit",
      description_label: "Descrizione",
      description_placeholder: "Breve riepilogo mostrato nel selettore di installazione.",
      prompt_label: "Prompt",
      prompt_placeholder: "Cerca problemi di sicurezza in sospeso in questo progetto…",
      prompt_help:
        "Il prompt di base che l'agente esegue a ogni tick. Il contesto per progetto viene aggiunto al momento dell'installazione, quindi mantieni questo prompt indipendente dal progetto.",
      enabled_label: "Abilitato",
      enabled_help:
        "Gli scheduler disabilitati non possono essere installati su nuovi progetti e i binding esistenti non verranno attivati.",
      cancel: "Annulla",
      saving: "Salvataggio…",
      save: "Salva",
      creating: "Creazione…",
      create: "Crea scheduler",
    },
  },
  power_k: {
    search_menu: {
      no_results: "Nessun risultato trovato",
      clear_search: "Cancella ricerca",
    },
    miscellaneous_actions: {
      copy_current_page_url_toast_success: "URL della pagina corrente copiato negli appunti.",
      copy_current_page_url_toast_error:
        "Si è verificato un errore durante la copia dell'URL della pagina corrente negli appunti.",
    },
    preferences_actions: {
      toast: {
        theme: {
          error: "Impossibile aggiornare il tema. Riprova.",
        },
        timezone: {
          success: "Fuso orario aggiornato con successo.",
          error: "Impossibile aggiornare il fuso orario. Riprova.",
        },
        generic: {
          success: "Preferenze aggiornate con successo.",
          error: "Impossibile aggiornare le preferenze. Riprova.",
        },
      },
    },
    footer: {
      workspace_level: "Livello workspace",
    },
    page_placeholders: {
      default: "Digita un comando o cerca",
    },
    contextual_actions: {
      cycle: {
        copy_url_toast_success: "URL del ciclo copiato negli appunti.",
        copy_url_toast_error: "Si è verificato un errore durante la copia dell'URL del ciclo negli appunti.",
      },
      module: {
        copy_url_toast_success: "URL del modulo copiato negli appunti.",
        copy_url_toast_error: "Si è verificato un errore durante la copia dell'URL del modulo negli appunti.",
      },
      page: {
        copy_url_toast_success: "URL della pagina copiato negli appunti.",
        copy_url_toast_error: "Si è verificato un errore durante la copia dell'URL della pagina negli appunti.",
      },
      work_item: {
        copy_id_toast_success: "ID dell'elemento di lavoro copiato negli appunti.",
        copy_id_toast_error:
          "Si è verificato un errore durante la copia dell'ID dell'elemento di lavoro negli appunti.",
        copy_title_toast_success: "Titolo dell'elemento di lavoro copiato negli appunti.",
        copy_title_toast_error:
          "Si è verificato un errore durante la copia del titolo dell'elemento di lavoro negli appunti.",
        copy_url_toast_success: "URL dell'elemento di lavoro copiato negli appunti.",
        copy_url_toast_error:
          "Si è verificato un errore durante la copia dell'URL dell'elemento di lavoro negli appunti.",
      },
    },
  },
  date: "",
  epics: "",
  Unassigned: "",
  creating_theme: "",
  issue_advanced_git: "",
  git_work_branch: "",
  git_work_branch_hint: "",
  git_work_branch_too_long: "",
  git_work_branch_invalid_chars: "",
  git_work_branch_placeholder: "",
  progress: "",
  customize_navigation: "Personalizza navigazione",
  personal: "Personale",
  accordion_navigation_control: "Navigazione laterale a fisarmonica",
  horizontal_navigation_bar: "Navigazione a schede",
  show_limited_projects_on_sidebar: "Mostra un numero limitato di progetti nella barra laterale",
  enter_number_of_projects: "Inserisci il numero di progetti",
  repo_url_too_long: "",
  git_repository_url_placeholder: "",
  base_branch_too_long: "",
  base_branch_invalid_chars: "",
  base_branch_placeholder: "",
  git_repository_url: "",
  base_branch: "",
  timezone_setting: "Impostazione del fuso orario corrente.",
  language_setting: "Scegli la lingua utilizzata nell'interfaccia utente.",
  language_and_time: "Lingua e ora",
  preferences: "Preferenze",
} as const;
