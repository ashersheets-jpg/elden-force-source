------------------------------------------
-- Elden Force additions to the player's behaviour script (loaded by c0000.hks after the game's own).
-- env() and act() are the game's; the rest is plain Lua (the game's script engine is minimal, and any
-- undefined name reads as a do-nothing function, so every variable here is set before use).
------------------------------------------

-- The game's script has its env() / act() ids built in as numbers; these are the ones used here.
IsLanding = 201
GetSpEffectID = 1116
IsCOMPlayer = 1007
TurnToLockonTargetImmediately = 2019
FaceDirection = 2029
GetMoveAnimParamID = 257
IsSummoningRide = 504
GetStamina = 1001
ActionDuration = 1108
MovementRequestDuration = 1111

------------------------------------------
-- Animation canceling (Elden Force)
------------------------------------------
-- Frames (60 per second) into an attack before a dodge can cut it short.
JR_CANCEL_AFTER = 16

JR_Frame = 0
JR_AttackFrom = -1000
JR_DodgeHeld = false
JR_DodgePressed = false
JR_CancelDodge = FALSE

-- A fresh press of dodge this frame (the raw button, not the game's queued request).
function JR_ReadPresses()
    local held = env(ActionDuration, ACTION_ARM_SP_MOVE) > 0
    JR_DodgePressed = held and not JR_DodgeHeld
    JR_DodgeHeld = held
end

-- Every attack start restarts the window.
local er_ExecAttack = ExecAttack
function ExecAttack(...)
    local started = er_ExecAttack(...)
    if started == TRUE then
        JR_AttackFrom = JR_Frame
    end
    return started
end

-- While cutting an attack short, the dodge is requested here: with the stick held a roll that way,
-- without it a backstep.
-- 1.2.0 round 4: a combo blow commits the fighter until it lands: no dodge before then (JR_Commit)
JR_CommitUntil = 0
local er_GetEvasionRequest = GetEvasionRequest
function GetEvasionRequest()
    if JR_Frame < JR_CommitUntil then
        return ATTACK_REQUEST_INVALID
    end
    if JR_CancelDodge == TRUE then
        if env(GetStamina) < STAMINA_MINIMUM then
            return ATTACK_REQUEST_INVALID
        end
        if env(MovementRequestDuration) > 0 then
            return ATTACK_REQUEST_ROLLING
        end
        return ATTACK_REQUEST_BACKSTEP
    end
    return er_GetEvasionRequest()
end

local er_AttackCommonFunction = AttackCommonFunction
function AttackCommonFunction(...)
    JR_CancelDodge = FALSE
    -- 1.2.0: not for the fighters: Carian Combo Warriors' moves have their own dodge windows (their
    -- heaviest and air moves can't be cut short, as its author meant)
    if JR_DodgePressed == true and JR_Frame - JR_AttackFrom >= JR_CANCEL_AFTER and JR_CCW_ON ~= true then
        JR_CancelDodge = TRUE
        c_HasActionRequest = TRUE
    end
    local result = er_AttackCommonFunction(...)
    JR_CancelDodge = FALSE
    return result
end

------------------------------------------
-- Air Boost (ELDEN VINS by mahku, with permission)
------------------------------------------
mod_AirBackstepFrameCount = 0
mod_AirBackstepCount = 0
mod_AirBackstepDisableHold = 0

function ModAirBackstep()
    if env(ActionDuration, ACTION_ARM_L2) > 0 and env(ActionDuration, ACTION_ARM_CHANGE_STYLE) > 0 then
        mod_AirBackstepDisableHold = mod_AirBackstepDisableHold + 1
    else
        mod_AirBackstepDisableHold = 0
    end

    if env(IsLanding) == FALSE and env(IsSummoningRide) == FALSE and mod_AirBackstepDisableHold < 9 then
        mod_AirBackstepFrameCount = mod_AirBackstepFrameCount + 1
        if mod_AirBackstepFrameCount > 10 and env(ActionDuration, ACTION_ARM_L2) > 0 and env(ActionDuration, ACTION_ARM_CHANGE_STYLE) > 0 and mod_AirBackstepCount < 1 then
            mod_AirBackstepCount = mod_AirBackstepCount + 1
            ModExecAirBackstep()
        end
    end
end

-- air boost
function ModExecAirBackstep()
    SetVariable("EvasionWeightIndex", EVASION_WEIGHT_INDEX_HEAVY)
    SetVariable("RollingDirectionIndex", 0)
    SetVariable("RollingAngleReal", 0)
    ExecEvent("W_Rolling")
end

function ModDoubleJump()
    if env(IsLanding) == TRUE then
        mod_AirBackstepFrameCount = 0
        mod_AirBackstepCount = 0
    end
end


------------------------------------------
-- Body animations for Elden Force's moves (Elden Force)
------------------------------------------
-- The mod's DLL can't start animations. For every move it writes one line of Lua to jr_cue.lua next to
-- this file (a call to one of the JR_ functions below) and applies one of two short marker SpEffects
-- (src/anim.rs); a new marker runs the file, which plays Elden Ring's own animation events. The DLL moves
-- the body meanwhile, so these are for the look of the move.
JR_CUE_A = 7280
JR_CUE_B = 102006
JR_LastCue = 0
JR_CueFile = JR_ScriptDir .. "jr_cue.lua"
-- 1.3.0: the Stands (a spirit summon running this same script) get their own cue file: the DLL marks the Stand with the
-- same markers and writes what it plays here (an Ash of War, src/stands.rs)
JR_NpcCueFile = JR_ScriptDir .. "jr_npc_cue.lua"
-- Gestures used as poses (GestureID; picked in the lab audition, lab/cues.txt): a hand thrust forward,
-- an arm raised high, both arms spread wide.
JR_G_CAST = 48
JR_G_RAISE = 51
JR_G_POWER = 24
JR_G_THROW = 51

-- Roll toward the stick: forward roll turned that way, or the matching side roll when locked on.
function JR_RollTowardStick()
    local angle = c_RollingAngle
    if GetVariable("IsLockon") == true then
        local index = 0
        if angle > 45 and angle < 135 then
            index = 3
        elseif angle < -45 and angle > -135 then
            index = 2
        elseif angle >= 135 or angle <= -135 then
            index = 1
        end
        SetVariable("RollingDirectionIndex", index)
    else
        SetVariable("RollingDirectionIndex", 0)
        if env(MovementRequestDuration) > 0 then
            act(FaceDirection, angle)
        end
    end
    SetVariable("EvasionWeightIndex", EVASION_WEIGHT_INDEX_LIGHT)
    ExecEvent("W_Rolling")
end

-- Air dash in the air, a quick forward dash on the ground.
function JR_Dash()
    if env(IsLanding) == FALSE then
        JR_RollTowardStick()
    else
        SetVariable("RollingDirectionIndex", 0)
        ExecEvent("W_Rolling")
    end
end

-- 0.14.3: the DLL landed the body itself (its flight, a swing) while this script still thinks it jumps
-- or falls: land it the game's way, so the game never decides the fighter is stuck in a fall
function JR_Land()
    JR_FlyUntil = 0
    ExecEvent("W_Jump_Land_N")
end

-- 0.16.0: down from a long fall or a skydive: Elden Ring's heavy landing (one knee down, a fist on
-- the ground: the superhero landing)
function JR_LandHard()
    JR_FlyUntil = 0
    SetVariable("LandIndex", LAND_HEAVY or 2)
    ExecEventAllBody("W_Land")
end

function JR_Leap()
    ExecEvent("W_Jump_N")
end

-- Elden Ring's plunging attack (heavy form): dive pose, quick impact landing.
function JR_Dive()
    SetVariable("JumpAttack_HandCondition", 0)
    SetVariable("JumpAttackForm", 2)
    SetVariable("JumpAttackFormRequest", 1)
    SetVariable("JumpAttack_Land", 0)
    ExecEventAllBody("W_JumpAttack_Start_Falling")
end

-- 1.2.0: the jump attack (light or heavy) out of one of the mod's own air poses (a shadow clone's throw,
-- the air dash's lean, the skydive), which take no attack themselves
function JR_AirAttack(heavy)
    -- 1.2.x (user: no charged heavy while falling, "the flying kick on fist moveset"): the hand condition as the
    -- game's own fall attack sets it (ExecFallAttack): the bare-handed fighters hold both fists (style both), whose
    -- jump heavy is Carian Combo Warriors' flying kick (a042_033230); 0 played the one-handed one (031230)
    local cond = 0
    if c_Style == HAND_RIGHT_BOTH or c_Style == HAND_LEFT_BOTH then
        cond = 1
    end
    SetVariable("JumpAttack_HandCondition", cond)
    SetVariable("JumpAttackForm", 0)
    if heavy then
        SetVariable("JumpAttackFormRequest", 1)
    else
        SetVariable("JumpAttackFormRequest", 0)
    end
    SetVariable("JumpAttack_Land", 0)
    ExecEventAllBody("W_JumpAttack_Start_Falling")
end

-- 1.2.0 (Todoroki's Dancing Flame): a heavy special played by the mod has no R2 held, so in its first
-- frames (the charge window) the script would cut it short into its quick ending. For `frames` updates the
-- two-handed heavy special's update is skipped, so the whole attack plays.
JR_SpecialHold = 0
JR_SpecialWrap = nil
function JR_HoldSpecial(frames)
    JR_SpecialHold = frames
    if AttackBothHeavySpecial1Start_onUpdate ~= JR_SpecialWrap then
        local orig = AttackBothHeavySpecial1Start_onUpdate
        JR_SpecialWrap = function(...)
            if JR_SpecialHold > 0 then
                JR_SpecialHold = JR_SpecialHold - 1
                return
            end
            return orig(...)
        end
        AttackBothHeavySpecial1Start_onUpdate = JR_SpecialWrap
    end
end

-- A pose from the gesture list (no gameplay effect).
function JR_Gesture(id)
    SetVariable("GestureID", id)
    ExecEventHalfBlend(Event_GestureStart, ALLBODY)
end

-- Weapons whose right-hand attack is a swing or a punch (not a bow, crossbow, catalyst, torch or shield).
-- 0.13.0: GetEquipWeaponCategory was never defined here, so this raised an error and every JR_Strike
-- did nothing (Asta's moves had no sword swings); the check now can't stop a strike.
GetEquipWeaponCategory = 225
function JR_MeleeInHand()
    local ok, melee = pcall(JR_MeleeCheck)
    return ok == false or melee ~= false
end
function JR_MeleeCheck()
    local kind = env(GetEquipWeaponCategory, HAND_RIGHT)
    if kind == WEAPON_CATEGORY_STAFF or kind == WEAPON_CATEGORY_SMALL_ARROW or kind == WEAPON_CATEGORY_ARROW
        or kind == WEAPON_CATEGORY_LARGE_ARROW or kind == WEAPON_CATEGORY_CROSSBOW or kind == WEAPON_CATEGORY_BALLISTA
        or kind == WEAPON_CATEGORY_SMALL_SHIELD or kind == WEAPON_CATEGORY_MIDDLE_SHIELD or kind == WEAPON_CATEGORY_LARGE_SHIELD
        or kind == WEAPON_CATEGORY_TORCH then
        return false
    end
    return true
end

-- A strike with the right hand: 1 = light, 2 = the second light swing, 3 = a heavy finisher.
-- With a bow, catalyst, torch or shield in the right hand: a thrust of the hand instead.
function JR_Strike(n)
    if JR_MeleeInHand() == false then
        JR_Gesture(JR_G_CAST)
        return
    end
    if n == 3 then
        ExecEventAllBody("W_AttackRightHeavy1Start")
    elseif n == 2 then
        ExecEventAllBody("W_AttackRightLight2")
    else
        ExecEventAllBody("W_AttackRightLight1")
    end
end

function JR_Kick()
    if JR_MeleeInHand() == false then
        JR_Gesture(JR_G_CAST)
        return
    end
    ExecEventAllBody("W_AttackRightBackstep")
end

-- Elden Ring's Ash of War on the empty hand: the DLL sets which one (EquipParamWeapon 110000's
-- swordArtsParamId) and this plays it the way the game does when L2 is pressed.
JR_ForceArts = false
-- 1.2.0: the fighters' attack buttons play their combo trees (the DLL's combos.rs), so Elden Ring's own
-- attacks on RB / RT / LB (its punch chain, jump and rolling attacks) stay quiet while this is on. The
-- DLL sets it for every fighter but Your Tarnished, who keeps their own weapons. L2 (the parry) and
-- the Ashes of War the DLL plays are untouched.
JR_OwnCombos = false
local er_GetAttackRequest = GetAttackRequest
function GetAttackRequest(...)
    if JR_ForceArts == true then
        JR_ForceArts = false
        return GetSwordArtsRequestNew()
    end

    local request = er_GetAttackRequest(...)
    if JR_OwnCombos == true and (request == ATTACK_REQUEST_RIGHT_LIGHT or request == ATTACK_REQUEST_RIGHT_HEAVY
        or request == ATTACK_REQUEST_LEFT_LIGHT or request == ATTACK_REQUEST_LEFT_HEAVY
        or request == ATTACK_REQUEST_BOTH_LIGHT or request == ATTACK_REQUEST_BOTH_HEAVY
        or request == ATTACK_REQUEST_BOTH_LEFT or request == ATTACK_REQUEST_DUAL_RIGHT
        or request == ATTACK_REQUEST_DUAL_LEFT) then
        return ATTACK_REQUEST_INVALID
    end
    return request
end

function JR_Combos(on)
    JR_OwnCombos = on
end

function JR_Commit(frames)
    JR_CommitUntil = JR_Frame + frames
end

function JR_Arts()
    c_SwordArtsID, c_SwordArtsHand = GetSwordArtInfo()
    JR_ForceArts = true
    ExecAttack("W_AttackRightLight1", "W_AttackRightHeavy1Start", "W_AttackLeftLight1", "W_AttackLeftHeavy1",
        "W_AttackBothLight1", "W_AttackBothHeavy1Start", FALSE, ALLBODY, FALSE, FALSE, FALSE)
    JR_ForceArts = false
end

-- 0.10.0: while the DLL flies the body (air dash, Superman flight, Black Whip swing) it tells this script
-- (a cue: JR_FlyFor), and the game's fall doesn't start: it would cut the flight's own animation within
-- a few frames. The frames are a safety limit in case the "landed" cue is missed.
JR_FlyUntil = 0
function JR_FlyFor(frames)
    JR_FlyUntil = JR_Frame + frames
end
local er_ExecFallStart = ExecFallStart
function ExecFallStart(...)
    if JR_Frame < JR_FlyUntil then
        return FALSE
    end
    return er_ExecFallStart(...)
end

function JR_Cues()
    local a = env(GetSpEffectID, JR_CUE_A) == TRUE
    local b = env(GetSpEffectID, JR_CUE_B) == TRUE
    local cue = 0
    if a then
        cue = 1
    elseif b then
        cue = 2
    end
    if cue ~= JR_LastCue and cue ~= 0 then
        local file = JR_CueFile
        if env(IsCOMPlayer) == TRUE then
            file = JR_NpcCueFile
        end
        local f = loadfile(file)
        if f ~= nil then
            f()
        end
    end
    JR_LastCue = cue
end

-- 0.14.3: falls never kill in Elden Force, but the game still played its lethal-fall sequence past a
-- certain height: the camera stops and watches the fighter fall away out of sight, and the death waits
-- for the landing. With the fall guard keeping them alive they stayed in it, unseen, until a move's
-- animation broke it. Every fall-death event becomes the ordinary fall or landing it stands for.
local JR_FALL_DEATH = {
    W_FallDeath = "W_FallLoop",
    W_FallDeathFaceUp = "W_LandFaceUp",
    W_FallDeathFaceDown = "W_LandFaceDown",
}
local function JR_NoFallDeath(e)
    -- (reading a table with any key is safe; only event names are in it)
    return JR_FALL_DEATH[e] or e
end
-- (plain global assignments: no _G, select or unpack in this script engine)
if ExecEvent ~= nil then
    local er = ExecEvent
    ExecEvent = function(e, a, b, c) return er(JR_NoFallDeath(e), a, b, c) end
end
if ExecEventAllBody ~= nil then
    local er = ExecEventAllBody
    ExecEventAllBody = function(e, a, b, c) return er(JR_NoFallDeath(e), a, b, c) end
end
if ExecEventNoReset ~= nil then
    local er = ExecEventNoReset
    ExecEventNoReset = function(e, a, b, c) return er(JR_NoFallDeath(e), a, b, c) end
end
if ExecEventSync ~= nil then
    local er = ExecEventSync
    ExecEventSync = function(e, a, b, c) return er(JR_NoFallDeath(e), a, b, c) end
end
if ExecEventSyncNoReset ~= nil then
    local er = ExecEventSyncNoReset
    ExecEventSyncNoReset = function(e, a, b, c) return er(JR_NoFallDeath(e), a, b, c) end
end
if ExecEventHalfBlend ~= nil then
    local er = ExecEventHalfBlend
    ExecEventHalfBlend = function(e, a, b, c) return er(JR_NoFallDeath(e), a, b, c) end
end
if ExecEventHalfBlendNoReset ~= nil then
    local er = ExecEventHalfBlendNoReset
    ExecEventHalfBlendNoReset = function(e, a, b, c) return er(JR_NoFallDeath(e), a, b, c) end
end

------------------------------------------
-- Main update
------------------------------------------
local er_Update = Update
function Update()
    JR_Frame = JR_Frame + 1
    pcall(JR_ReadPresses)
    er_Update()
    pcall(ModDoubleJump)
    pcall(ModAirBackstep)
    pcall(JR_Cues)
end
