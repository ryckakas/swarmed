// What things are called, by the style the user chose: the plain words, or a swarm's. Apart from
// the palette, so either goes with any colours. Pure, so it can be tested.

import type {
    BackgroundCount,
    BackgroundState,
    SessionStatus,
    StackState,
    Style,
} from "./api/types";
import { DEFAULT_STYLE } from "./appearance";

/** What a session is doing, as the marks tell it: its status, or idle with an unseen answer. */
export type StatusKind = SessionStatus | "unread";

/** What a handful of things are called. */
export interface Words {
    /** The small label over the tree of repositories; nothing in the plain style. */
    overview: string;
    /** The question on the new-task screen. */
    newTask: string;
    /** What the button that starts a session in a task is called. */
    newSession: string;
    /** What stands under a task that has no sessions. */
    noSessions: string;
    /**
     * What each status is called. Every one is a list: with more than one word, the app shows
     * one of them at random and changes it now and then. Add words here to add variety.
     */
    status: Record<StatusKind, string[]>;
    /**
     * Other words for what the agent is doing right now, by the plain word: lists, like the
     * statuses. What is not named here, such as which tool runs, is shown as it is.
     */
    activity: Record<string, string[]>;
    /** How many sessions wait for the user, in the strip over the tree. */
    needYou: (count: number) => string;
    /** How many sessions hold an answer nobody has looked at. */
    unread: (count: number) => string;
    /** What stands under a repository that has no tasks. */
    noTasks: string;
    /** The button that starts a task. */
    startTask: string;
    /** The button that adds a repository. */
    addRepository: string;
    /** The line that unfolds a task's closed sessions. */
    closed: (count: number) => string;
    /** The button that puts a closed session back. */
    reopen: string;
    /** The line that unfolds a repository's tasks that were put away. */
    done: (count: number) => string;
    /** The line that unfolds a task's snoozed sessions. */
    snoozed: (count: number) => string;
    /** The button that wakes a snoozed session before its time. */
    wake: string;
    /** What handing work to a cloud agent is called, before "from <where>". */
    delegate: string;
    /** Taking a cloud task off the list, which leaves it running at its provider. */
    forgetCloud: string;
    /** The context card's parts on the agent's usage windows and on what the session spent. */
    usageLimits: string;
    spentHere: string;
    /** The context card's part on the account's extra usage, billed beyond the plan. */
    extraUsage: string;
    /** The overview of the plan's usage that the coins button in a session's header opens. */
    usage: string;
    /** Its button that asks Claude Code for the figures again. */
    askAgain: string;
    /** That button while the figures are being asked for. */
    asking: string;
    /** The button on a command Claude Code has a screen for only in its own terminal. */
    openInClaude: string;
    /** The button that ends that Claude Code and shows the session's shell again. */
    backToShell: string;
    /** What ending a session and taking it out of the sidebar is called. */
    closeSession: string;
    /** Starting a session from all of this one, on the same agent and on another. */
    forkAll: string;
    cloneTo: string;
    /** Closing a session and keeping its conversation, and deleting it with the conversation. */
    archiveSession: string;
    deleteSession: string;
    /** The question asked before a session is closed. */
    closeAsk: string;
    /** One way of setting a task up, and several of them. */
    environment: string;
    environments: string;
    /** How a turn's end reads: one that finished, and one that was stopped or failed. */
    turnDone: string;
    turnStopped: string;
    /** What each state of a task's stack is called. */
    stack: Record<StackState, string>;
    /** The status line's piece that adds up how long the agent has worked. */
    timeWorked: string;
    /** The task panel's two tabs: what the task changed, and what its agents left running. */
    files: string;
    /** An agent a session's agent started for part of its work, and going back from one. */
    subAgent: string;
    subAgentWorking: string;
    subAgentDone: string;
    backToSession: string;
    changes: string;
    background: string;
    /** What each state of something left running is called. */
    backgroundState: Record<BackgroundState, string>;
    /** The button that stops something left running. */
    stopBackground: string;
    /** What the background tab says when nothing was left running. */
    noBackground: string;
    /** The heading over the charts of what the sessions did. */
    activityTitle: string;
    /** The switch that asks an agent for its fast mode. */
    fast: string;
    /** What a task's setup produced, kept for new tasks to start from; and several. */
    snapshot: string;
    snapshots: string;
    /** The buttons that unfold and fold every changed file at once. */
    expandAll: string;
    collapseAll: string;
    /** The button in the commit box that has a message written for the changes. */
    writeMessage: string;
    /** The button in the pull request form that has its title and description written. */
    writePull: string;
    /** The row of the user's own buttons on a task, and the menu holding those that do not fit. */
    actions: string;
    moreActions: string;
    /** What the version in the corner opens, and what the About view is called. */
    about: string;
    /** The installed version: short, as the corner shows it, and as the About view names it. */
    versionTag: (version: string) => string;
    versionName: (version: string) => string;
    /** What Swarmed is, in a sentence, in the About view. */
    aboutLine: string;
    /** Updating Swarmed, in the About view and its settings. */
    update: {
        /** While it looks for a new version. */
        checking: string;
        /** The installed version is the newest. */
        upToDate: string;
        /** The button that looks for a new version. */
        check: string;
        /** A new version is out, and the button that installs it and restarts. */
        available: (version: string) => string;
        install: string;
        /** A new version coming in, with how far it got, such as "40%". */
        downloading: (version: string, progress: string) => string;
        /** A new version downloaded and checked, and the button that restarts into it. */
        ready: (version: string) => string;
        restart: string;
        /** The setting that has Swarmed update by itself. */
        automatic: string;
    };
    /** The user's preferences: the button and window, and a pointer to it. */
    settings: string;
    installInSettings: string;
    /** The list of keyboard shortcuts. */
    shortcuts: string;
    /** The button that adds one of the user's own buttons. */
    addAction: string;
    /** A session, in the middle of a sentence: "a new session", "in any session". */
    session: string;
    /** A session by its number, at the start of a line or alone. */
    sessionNumbered: (id: number) => string;
    /** What a session that has no title yet is called. */
    untitled: string;
    /** The line under a session another session started. */
    startedBy: (id: number) => string;
    /** Whom a session works for, by that one's number, and the menu's choice of nobody. */
    worksFor: (id: number) => string;
    worksForNobody: string;
    /** What stands before the session the one on screen works for, and before those that work for it. */
    worksForHeading: string;
    workingFor: string;
    /** A role by its name, as a menu or a line says it; one the window does not know, as named. */
    role: (name: string) => string;
    /** A role's short form, on a sidebar row. */
    roleShort: (name: string) => string;
    /** The role menu's name, and its choice of none. */
    roleMenu: string;
    noRole: string;
    /**
     * Why the supervision menu of a new session is fixed while its role, as `role` says it,
     * holds it in plan mode, and what its plan's approval does instead of the usual.
     */
    roleHolds: (role: string) => string;
    planKept: (role: string) => string;
    /** What approving the plan of a role that hands it on in `mode`, a supervision's name, does. */
    planHandedOn: (role: string, mode: string) => string;
    /**
     * What a session, or the sessions under a folded heading, have running in the background,
     * as its status mark says when pointed at: "2 sub-agents, 1 command in the background".
     */
    inBackground: (count: BackgroundCount) => string;
    /** The transcript's button that starts a session from one message on, and what it does. */
    forkHere: string;
    forkHereHint: string;
    /** What a session started from part of another says at its top. */
    forkedFrom: (id: number) => string;
    /**
     * The transcript's button that takes the session back to before a message to edit it, and
     * what it does.
     */
    editHere: string;
    editHereHint: string;
    /** A message another session sent, by that session's number. */
    fromSession: (id: number) => string;
    /** While a session's conversation is read. */
    loadingConversation: string;
    /** What a session says before anything was said in it. */
    emptySession: string;
    /** The message box's placeholder while the agent waits for the next message. */
    reply: string;
    /** The new-task screen: what sending does, and what is going on after. */
    newTaskHint: string;
    namingTask: string;
    settingUp: (name: string) => string;
    /** Its box's placeholder, and the picker of the repository it goes in. */
    describeTask: string;
    repository: string;
    /**
     * The chats, sessions with no repository: what the picker offers in place of one, the
     * new-task screen's hint and placeholder for one, and the heading they are listed under.
     */
    noRepository: string;
    newChatHint: string;
    describeChat: string;
    chats: string;
    /** What a chat is, and starting a task in a repository that carries on from one. */
    chat: string;
    carryOn: string;
    /** The picker that runs a new task's prompt on another agent too. */
    alsoRunOn: string;
    /** A task set up with no environment: the repository's own setup. */
    ownSetup: string;
    /** The changes tab: while they are read, and when there are none. */
    readingChanges: string;
    noChanges: (base: string) => string;
    /** Its button that hands the review comments to the session, and what that does. */
    sendToSession: string;
    sendToSessionHint: string;
    /** Comparing tasks: a file one did not change, the picker, and a cloud task's changes. */
    notChanged: string;
    notChangedHere: string;
    compareTask: string;
    askingChanges: string;
    /** The files tab when the worktree holds nothing to list. */
    noFiles: string;
    /** Something left running by a sub-agent, and the button to the session that started it. */
    backgroundAgent: string;
    backgroundOwner: string;
    /** In place of the stop button, for what its agent offers no way to stop. */
    backgroundUnstoppable: string;
    /** A sub-agent's pane: before it has said anything, and over what it answered. */
    subAgentSilent: string;
    subAgentAnswer: string;
    /** The sidebar with no repositories, and the toggle that folds a repository's tasks. */
    noRepositories: string;
    showTasks: string;
    hideTasks: string;
    /** The heading that folds a task's sessions, after what it says of the task's worktree. */
    showSessions: string;
    hideSessions: string;
    /** The cloud dialog's button that hands the work over, and while it does. */
    delegateButton: string;
    delegating: string;
    /** The list of what waits for the user, and when nothing does. */
    waitingForYou: string;
    nothingWaits: string;
    /** The search box's placeholder. */
    searchPlaceholder: string;
    /** What the activity charts count a turn as. */
    turns: string;
    /** A stack's button while it starts. */
    stackStarting: string;
    /**
     * What things are called, in lower case as they stand in a sentence; `capitalized` makes a
     * label of one. Only the colony style names these its own way: git's and the agents' words
     * included, which the swarm dictionary explains.
     */
    terms: {
        repository: string;
        repositories: string;
        task: string;
        tasks: string;
        worktree: string;
        worktrees: string;
        branch: string;
        branches: string;
        baseBranch: string;
        /** A commit, and to commit; several, while it is done, and done. */
        commit: string;
        commits: string;
        committing: string;
        committed: string;
        /** To push a branch; while it is done, and done. */
        push: string;
        pushing: string;
        pushed: string;
        pullRequest: string;
        agent: string;
        agents: string;
        model: string;
        models: string;
        mcpServers: string;
        skill: string;
        skills: string;
        prompt: string;
        prompts: string;
        rules: string;
    };
}

/** The built-in roles' short forms, for a sidebar row; any other role is shown by its name. */
const PLAIN_ROLES_SHORT: Partial<Record<string, string>> = {
    architect: "arch",
    planner: "plan",
    implementer: "impl",
    reviewer: "review",
};

/** The built-in roles as a swarm calls them, whole and short. */
const SWARM_ROLES: Partial<Record<string, [string, string]>> = {
    architect: ["Queen", "queen"],
    planner: ["Broodkeeper", "brood"],
    implementer: ["Worker", "worker"],
    reviewer: ["Sentinel", "guard"],
};

const PLAIN: Words = {
    overview: "",
    newTask: "What should the agent work on?",
    newSession: "New session",
    noSessions: "No sessions",
    status: {
        idle: ["Idle"],
        unread: ["Unread reply"],
        working: ["Working"],
        waiting: ["Needs you"],
        errored: ["Stopped with an error"],
    },
    activity: {},
    needYou: (count) => (count === 1 ? "1 session needs you" : `${count} sessions need you`),
    unread: (count) => `${count} unread`,
    noTasks: "No tasks",
    startTask: "New task",
    addRepository: "Add repository",
    closed: (count) => `${count} closed`,
    reopen: "Reopen",
    done: (count) => `${count} done`,
    snoozed: (count) => `${count} snoozed`,
    wake: "Wake",
    delegate: "Delegate to the cloud",
    forgetCloud: "Take off the list",
    usageLimits: "Usage limits",
    spentHere: "Spent in this session",
    extraUsage: "Extra usage",
    usage: "Usage",
    askAgain: "Ask again",
    asking: "Asking…",
    openInClaude: "Open in Claude Code",
    backToShell: "Back to the shell",
    closeSession: "Close session",
    forkAll: "Fork this conversation",
    cloneTo: "Clone to…",
    archiveSession: "Archive",
    deleteSession: "Delete",
    closeAsk: "Close this session?",
    environment: "Environment",
    environments: "Environments",
    turnDone: "Turn finished",
    turnStopped: "Turn stopped",
    stack: { stopped: "Stack stopped", starting: "Stack starting", running: "Stack running" },
    timeWorked: "Time worked",
    files: "Files",
    subAgent: "Sub-agent",
    subAgentWorking: "At work",
    subAgentDone: "Done",
    backToSession: "Back to the session",
    changes: "Changes",
    background: "Background",
    backgroundState: {
        running: "Running",
        completed: "Finished",
        failed: "Failed",
        stopped: "Stopped",
    },
    stopBackground: "Stop",
    noBackground: "The agents of this task have left nothing running.",
    activityTitle: "Activity",
    fast: "Fast",
    snapshot: "Snapshot",
    snapshots: "Snapshots",
    expandAll: "Expand all",
    collapseAll: "Collapse all",
    writeMessage: "Generate a commit message",
    writePull: "Generate the title and description",
    actions: "Actions",
    moreActions: "More actions…",
    about: "About Swarmed",
    versionTag: (version) => `v${version}`,
    versionName: (version) => `Version ${version}`,
    aboutLine: "Coding agents side by side, each on a branch and in a worktree of its own.",
    update: {
        checking: "Checking for updates…",
        upToDate: "Swarmed is up to date.",
        check: "Check for updates",
        available: (version) => `Swarmed ${version} is out.`,
        install: "Update and restart",
        downloading: (version, progress) => `Downloading Swarmed ${version}… ${progress}`,
        ready: (version) => `Swarmed ${version} is downloaded, and goes in when you quit.`,
        restart: "Restart to update",
        automatic: "Update automatically",
    },
    settings: "Settings",
    installInSettings: "Install it in Settings",
    shortcuts: "Keyboard shortcuts",
    addAction: "Add an action",
    session: "session",
    sessionNumbered: (id) => `Session ${id}`,
    untitled: "New session",
    startedBy: (id) => `Started by session ${id}`,
    worksFor: (id) => `Works for session ${id}`,
    worksForNobody: "Works for nobody",
    worksForHeading: "Works for",
    workingFor: "Working for it",
    role: (name) => capitalized(name),
    roleShort: (name) => PLAIN_ROLES_SHORT[name] ?? name,
    roleMenu: "Role",
    noRole: "No role",
    roleHolds: (role) =>
        `${role}: starts in plan mode, or its agent's read-only mode, and changes no files. Change it once the session runs.`,
    planKept: (role) =>
        `${role}: once approved, it stays in plan mode and hands the plan on, changing no files itself.`,
    planHandedOn: (role, mode) =>
        `${role}: once approved, plan mode ends (${mode.toLowerCase()}) so that it starts and follows the sessions that carry the plan out, changing no files itself.`,
    inBackground: ({ agents, commands }) =>
        `${counted([
            [agents, "sub-agent", "sub-agents"],
            [commands, "command", "commands"],
        ])} in the background`,
    forkHere: "Fork from here",
    forkHereHint:
        "Starts a new session with this message and everything said after it, and nothing from before",
    forkedFrom: (id) =>
        `Forked from session ${id}. What stands above was said there; this session's agent is given it with your first message.`,
    editHere: "Edit from here",
    editHereHint:
        "Takes this session back to before this message, its files too if you choose, and puts the message in the box to edit and send again",
    fromSession: (id) => `From session ${id}`,
    loadingConversation: "Loading the conversation…",
    emptySession: "Nothing has been said yet. The agent starts with your first message.",
    reply: "Reply to the agent",
    newTaskHint: "Creates a task for this in its own worktree.",
    newChatHint: "Starts a chat in a folder of its own, with no repository.",
    namingTask: "Naming the task…",
    settingUp: (name) => `Setting up ${name}…`,
    describeTask: "Describe the task",
    repository: "Repository",
    noRepository: "No repository",
    describeChat: "Ask anything",
    chats: "General",
    chat: "chat",
    carryOn: "Carry on in a repository",
    alsoRunOn: "Also run on…",
    ownSetup: "Repository's own setup",
    readingChanges: "Reading the changes…",
    noChanges: (base) => `Nothing has changed since this branch left ${base}.`,
    sendToSession: "Send to this session",
    sendToSessionHint: "Sends the comments to the session on screen, as one message",
    notChanged: "not changed",
    notChangedHere: "Not changed in this task.",
    compareTask: "Compare with another task",
    askingChanges: "Asking for the changes…",
    noFiles: "Nothing here yet.",
    backgroundAgent: "Agent",
    backgroundOwner: "Show the session that started it",
    backgroundUnstoppable: "Its agent offers no way to stop it from here.",
    subAgentSilent: "Nothing heard from it yet.",
    subAgentAnswer: "What it handed back",
    noRepositories: "No repositories yet.",
    showTasks: "Show this repository's tasks",
    hideTasks: "Hide its tasks",
    showSessions: "Show this task's sessions",
    hideSessions: "Hide its sessions",
    delegateButton: "Delegate",
    delegating: "Delegating…",
    waitingForYou: "Waiting for you",
    nothingWaits: "Nothing waits for you.",
    searchPlaceholder: "Search what was said, in any session",
    turns: "Turns",
    stackStarting: "Starting…",
    terms: {
        repository: "repository",
        repositories: "repositories",
        task: "task",
        tasks: "tasks",
        worktree: "worktree",
        worktrees: "worktrees",
        branch: "branch",
        branches: "branches",
        baseBranch: "base branch",
        commit: "commit",
        commits: "commits",
        committing: "committing",
        committed: "committed",
        push: "push",
        pushing: "pushing",
        pushed: "pushed",
        pullRequest: "pull request",
        agent: "agent",
        agents: "agents",
        model: "model",
        models: "models",
        mcpServers: "MCP servers",
        skill: "skill",
        skills: "skills",
        prompt: "prompt",
        prompts: "prompts",
        rules: "rules",
    },
};

const SWARM: Words = {
    ...PLAIN,
    overview: "Hive",
    newTask: "What should the swarm work on?",
    newSession: "Spawn a session",
    noSessions: "Nothing spawned yet",
    status: {
        idle: ["Dormant"],
        unread: ["Hatched"],
        working: ["Mutating", "Incubating"],
        // These two ask something of the user, so they stay plain in every style.
        waiting: ["Needs you"],
        errored: ["Stopped with an error"],
    },
    activity: {
        Starting: ["Stirring", "Waking"],
        Thinking: ["Gestating", "Digesting", "Brooding"],
        Writing: ["Secreting", "Spinning"],
        "Presenting its plan": ["Unfurling its plan"],
    },
    needYou: (count) => (count === 1 ? "1 awaits you" : `${count} await you`),
    unread: (count) => `${count} hatched`,
    noTasks: "The nest is empty",
    closeSession: "Terminate spawn",
    forkAll: "Split this spawn",
    cloneTo: "Clone onto…",
    archiveSession: "Burrow",
    deleteSession: "Cull",
    closeAsk: "Terminate this spawn?",
    environment: "Spawning pool",
    environments: "Spawning pools",
    turnDone: "Molt complete",
    turnStopped: "Molt cut short",
    stack: { stopped: "Pool dormant", starting: "Pool gestating", running: "Pool thriving" },
    timeWorked: "Time mutating",
    files: "Tunnels",
    subAgent: "Hatchling",
    subAgentWorking: "Foraging",
    subAgentDone: "Returned",
    backToSession: "Back to the spawn",
    changes: "Mutations",
    background: "Drones",
    backgroundState: {
        running: "Foraging",
        completed: "Returned",
        // Something went wrong, which the user should not have to decode.
        failed: "Failed",
        stopped: "Culled",
    },
    stopBackground: "Cull",
    noBackground: "No drones are out for this task.",
    activityTitle: "Hive activity",
    fast: "Frenzy",
    snapshot: "Cocoon",
    snapshots: "Cocoons",
    expandAll: "Unburrow all",
    collapseAll: "Burrow all",
    writeMessage: "Spin a commit message",
    writePull: "Spin the title and description",
    actions: "Instincts",
    moreActions: "More instincts…",
    about: "About the hive",
    versionTag: (version) => `e${version}`,
    versionName: (version) => `Evolution ${version}`,
    aboutLine:
        "A swarm of coding agents at work side by side, each on a branch and in a worktree of its own.",
    update: {
        checking: "Sensing the next evolution…",
        upToDate: "The hive is fully evolved.",
        check: "Sense the next evolution",
        available: (version) => `Evolution ${version} has emerged.`,
        install: "Evolve and restart",
        downloading: (version, progress) => `Gestating evolution ${version}… ${progress}`,
        ready: (version) => `Evolution ${version} has gestated, and takes hold when you quit.`,
        restart: "Restart to evolve",
        automatic: "Evolve automatically",
    },
    settings: "Evolution Chamber",
    installInSettings: "Install it in the Evolution Chamber",
    shortcuts: "Reflexes",
    addAction: "Add an instinct",
    session: "spawn",
    sessionNumbered: (id) => `Spawn ${id}`,
    untitled: "New spawn",
    startedBy: (id) => `Sent out by spawn ${id}`,
    worksFor: (id) => `Serves spawn ${id}`,
    worksForNobody: "Serves no spawn",
    worksForHeading: "Serves",
    workingFor: "Serving it",
    role: (name) => SWARM_ROLES[name]?.[0] ?? capitalized(name),
    roleShort: (name) => SWARM_ROLES[name]?.[1] ?? name,
    roleMenu: "Caste",
    noRole: "No caste",
    roleHolds: (role) =>
        `${role}: starts in plan mode, or its agent's read-only mode, and touches nothing. Change it once the spawn hatches.`,
    planKept: (role) =>
        `${role}: once approved, it stays in plan mode and passes the plan to the swarm, touching nothing itself.`,
    planHandedOn: (role, mode) =>
        `${role}: once approved, plan mode ends (${mode.toLowerCase()}) so that it hatches and tends the swarm that carries the plan out, touching nothing itself.`,
    inBackground: ({ agents, commands }) =>
        [
            agents > 0 ? `${counted([[agents, "hatchling", "hatchlings"]])} foraging` : "",
            commands > 0 ? `${counted([[commands, "command", "commands"]])} left running` : "",
        ]
            .filter((part) => part !== "")
            .join(", "),
    forkHere: "Split from here",
    forkHereHint:
        "Spawns from this message: everything said after it comes along, and nothing from before",
    forkedFrom: (id) =>
        `Split from spawn ${id}. What stands above was said there; this spawn's agent is given it with your first message.`,
    editHere: "Rewind to here",
    editHereHint:
        "Takes this spawn back to before this message, its files too if you choose, and puts the message back in the box to send again",
    fromSession: (id) => `From spawn ${id}`,
    loadingConversation: "Waking the spawn…",
    emptySession: "Nothing has stirred yet. The spawn wakes with your first message.",
    reply: "Reply to the spawn",
    settingUp: (name) => `Incubating ${name}…`,
    alsoRunOn: "Also spawn on…",
    ownSetup: "Colony's own pool",
    readingChanges: "Reading the mutations…",
    noChanges: (base) => `No mutations since this branch left ${base}.`,
    sendToSession: "Send to this spawn",
    sendToSessionHint: "Sends the comments to the spawn on screen, as one message",
    notChanged: "no mutations",
    notChangedHere: "No mutations in this task.",
    askingChanges: "Asking for the mutations…",
    noFiles: "These tunnels are empty.",
    backgroundAgent: "Hatchling",
    backgroundOwner: "Show the spawn that sent it out",
    backgroundUnstoppable: "Its spawn cannot call this drone back.",
    subAgentSilent: "No word from the hatchling yet.",
    subAgentAnswer: "What it brought back",
    waitingForYou: "Awaiting you",
    nothingWaits: "Nothing awaits you.",
    searchPlaceholder: "Search what was said, in any spawn",
    showSessions: "Show this task's spawns",
    hideSessions: "Hide its spawns",
    turns: "Molts",
    stackStarting: "Waking…",
};

/** The swarm's words, and its names for the tree as well. */
const COLONY: Words = {
    ...SWARM,
    startTask: "New brood",
    addRepository: "Found a colony",
    closed: (count) => `${count} burrowed`,
    reopen: "Unburrow",
    // A cell whose brood is grown is capped with wax.
    done: (count) => `${count} sealed`,
    snoozed: (count) => `${count} hibernating`,
    wake: "Rouse",
    delegate: "Send spores to the cloud",
    forgetCloud: "Let it drift",
    usageLimits: "Hunger limits",
    spentHere: "Consumed by this spawn",
    extraUsage: "Borrowed biomass",
    usage: "Hunger",
    askAgain: "Sniff again",
    asking: "Sniffing…",
    openInClaude: "Open in Claude Code's lair",
    backToShell: "Back to the burrow",
    newTaskHint: "Lays a brood for this in its own worktree.",
    newChatHint: "Starts a murmur in a hollow of its own, with no colony.",
    namingTask: "Naming the brood…",
    describeTask: "Describe the brood",
    repository: "Colony",
    noRepository: "No colony",
    describeChat: "Murmur anything",
    chats: "Commons",
    chat: "murmur",
    carryOn: "Carry on in a colony",
    compareTask: "Compare with another brood",
    noRepositories: "No colonies yet.",
    showTasks: "Show this colony's broods",
    hideTasks: "Hide its broods",
    showSessions: "Show this brood's spawns",
    hideSessions: "Hide its spawns",
    delegateButton: "Send spores",
    delegating: "Sending spores…",
    terms: {
        repository: "colony",
        repositories: "colonies",
        task: "brood",
        tasks: "broods",
        worktree: "cell",
        worktrees: "cells",
        branch: "strain",
        branches: "strains",
        baseBranch: "mother strain",
        commit: "seal",
        commits: "seals",
        committing: "sealing",
        committed: "sealed",
        push: "release",
        pushing: "releasing",
        pushed: "released",
        pullRequest: "offering",
        agent: "species",
        agents: "species",
        model: "mind",
        models: "minds",
        mcpServers: "symbionts",
        skill: "gene",
        skills: "genes",
        prompt: "pheromone",
        prompts: "pheromones",
        rules: "hive law",
    },
};

const WORDS: Record<Style, Words> = { plain: PLAIN, swarm: SWARM, colony: COLONY };

/** The words that go with a style. */
export function wordsFor(style: Style | undefined): Words {
    return WORDS[style ?? DEFAULT_STYLE];
}

/** A word as a label starts: "worktree" as "Worktree". Names such as "MCP servers" stay as they are. */
export function capitalized(word: string): string {
    return word.charAt(0).toUpperCase() + word.slice(1);
}

/** One of the swarm's words, the plain word it stands for, and what that is, in plain words. */
export interface Term {
    swarm: string;
    plain: string;
    meaning: string;
}

/**
 * Every word the swarm's styles may call something otherwise, by where it is found in `Words`,
 * with what it is. The dictionary is made from these and the words themselves, so a word
 * renamed here is renamed there too.
 */
const ENTRIES: [pick: (words: Words) => string, meaning: string][] = [
    [(w) => w.overview || "Overview", "Everything Swarmed works on, in the sidebar."],
    [(w) => w.terms.repository, "A git repository you added to Swarmed."],
    [(w) => w.terms.task, "A piece of work, on a branch and in a worktree of its own."],
    [(w) => w.chats, "Where the chats are listed: sessions with no repository."],
    [(w) => w.chat, "A session with no repository, in a folder of its own."],
    [(w) => w.session, "A conversation with an agent, in a task."],
    [(w) => w.subAgent, "An agent that a session's agent started for part of its work."],
    [(w) => w.background, "Commands and agents a session left running."],
    [(w) => w.terms.worktree, "The folder a task works in: a git worktree of the repository."],
    [(w) => w.terms.branch, "A task's git branch."],
    [(w) => w.terms.baseBranch, "The branch a task starts from, and is compared with."],
    [(w) => w.terms.commit, "A git commit, and committing."],
    [(w) => w.terms.push, "Pushing a branch to the remote, such as GitHub."],
    [(w) => w.terms.pullRequest, "A pull request on GitHub."],
    [(w) => w.terms.agent, "A coding agent: Claude Code, Codex, OpenCode and the like."],
    [(w) => w.terms.model, "The model an agent runs on."],
    [(w) => w.terms.mcpServers, "Tools from outside that agents can use."],
    [(w) => w.terms.skills, "Instructions an agent takes up when they fit the work."],
    [(w) => w.terms.prompts, "Prompts you saved, to start work from."],
    [(w) => w.terms.rules, "Your own rules, given to every agent."],
    [(w) => w.changes, "What a task changed."],
    [(w) => w.files, "A task's files."],
    [(w) => w.environment, "How a new task is set up."],
    [(w) => w.snapshot, "What a task's setup made, kept for new tasks to start from."],
    [(w) => w.actions, "Buttons of your own, on every task."],
    [(w) => w.settings, "Swarmed's settings."],
    [(w) => w.shortcuts, "The keyboard shortcuts."],
    [(w) => w.turns, "An agent's work on one message."],
    [(w) => w.archiveSession, "Closing a session, keeping its conversation."],
    [(w) => w.deleteSession, "Deleting a session and its conversation for good."],
    [(w) => w.fast, "An agent's fast mode."],
    [(w) => w.versionName("").trim(), "The installed version of Swarmed."],
    [(w) => w.status.idle.join(", "), "A session waiting for your next message."],
    [(w) => w.status.working.join(", "), "A session whose agent is at work."],
    [(w) => w.status.unread.join(", "), "A session with a reply you have not read."],
];

/** What the swarm's words in a style mean, those that differ from the plain ones, A to Z. */
export function dictionary(style: Style | undefined): Term[] {
    const words = wordsFor(style);
    return ENTRIES.map(([pick, meaning]) => ({
        swarm: capitalized(pick(words)),
        plain: capitalized(pick(PLAIN)),
        meaning,
    }))
        .filter((term) => term.swarm !== term.plain)
        .sort((a, b) => a.swarm.localeCompare(b.swarm));
}

/** The terms with what was typed in either word or in what they mean, ignoring case. */
export function searchTerms(terms: Term[], query: string): Term[] {
    const wanted = query.trim().toLowerCase();
    const has = (text: string) => text.toLowerCase().includes(wanted);
    return terms.filter((term) => has(term.swarm) || has(term.plain) || has(term.meaning));
}

/** A name as it stands in the middle of a sentence: "an environment", "a spawning pool". */
export function withArticle(name: string): string {
    const lower = name.toLowerCase();
    return `${/^[aeiou]/.test(lower) ? "an" : "a"} ${lower}`;
}

/** The words on offer for what the agent is doing: the swarm's, or the plain word alone. */
export function activityWords(words: Words, plain: string): string[] {
    return words.activity[plain] ?? [plain];
}

/** Several numbers of things, each with its word, the ones of none left out: "2 sub-agents, 1 command". */
function counted(things: [count: number, one: string, many: string][]): string {
    return things
        .filter(([count]) => count > 0)
        .map(([count, one, many]) => `${count} ${count === 1 ? one : many}`)
        .join(", ");
}

/** Which mark a session gets: unread only counts while it is idle, as the rest says more. */
export function statusKind(status: SessionStatus, unread: boolean): StatusKind {
    return unread && status === "idle" ? "unread" : status;
}

/** One of the words, chosen by a roll from 0 up to, not including, 1. */
export function pickWord(words: string[], roll: number): string {
    const index = Math.min(words.length - 1, Math.max(0, Math.floor(roll * words.length)));
    return words[index] ?? "";
}
