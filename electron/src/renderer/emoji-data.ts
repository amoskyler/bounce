/**
 * The emoji table. GENERATED — do not edit; run `npm run build:emoji`.
 *
 * Packed into one string rather than written as 1914 object literals
 * because the parsed form is what we want in memory anyway, and a megabyte of
 * JavaScript object syntax costs both bundle size and parse time to arrive at
 * the same place.
 *
 * Records are newline-separated. Fields are tab-separated, in order:
 * character, label, category index, shortcodes, keywords — the last two being
 * space-separated lists. Shortcodes are stored without their colons, and the
 * first one is the canonical name shown in the picker.
 */

/** One emoji, as the picker and the typeahead see it. */
export interface Emoji {
  /** The character itself, ready to insert. */
  readonly char: string;
  /** Human-readable name, e.g. "grinning face". */
  readonly label: string;
  /** Index into {@link EMOJI_CATEGORIES}. */
  readonly category: number;
  /** Names typeable between colons. Never empty; the first is canonical. */
  readonly shortcodes: readonly string[];
  /** Extra search terms that do not already appear in the label or shortcodes. */
  readonly keywords: readonly string[];
}

/** Picker sections, in display order. */
export const EMOJI_CATEGORIES: readonly string[] = ["Smileys & People","Animals & Nature","Food & Drink","Activities","Travel & Places","Objects","Symbols","Flags"];

const PACKED = `😀	grinning face	0	grinning grinning_face	cheerful cheery grin happy laugh nice smile smiling teeth
😃	grinning face with big eyes	0	smiley grinning_face_with_big_eyes	awesome grin happy mouth open smile smiling teeth yay
😄	grinning face with smiling eyes	0	smile grinning_face_with_closed_eyes	eye grin happy laugh lol mouth open
😁	beaming face with smiling eyes	0	grin beaming_face	eye grinning happy nice smile teeth
😆	grinning squinting face	0	laughing satisfied lol squinting_face	closed eyes haha hahaha happy laugh mouth open rofl smile smiling
😅	grinning face with sweat	0	sweat_smile grinning_face_with_sweat	cold dejected excited mouth nervous open smiling stress stressed
🤣	rolling on the floor laughing	0	rolling_on_the_floor_laughing rofl	crying face funny haha happy hehe hilarious joy laugh lmao lol roflmao tear
😂	face with tears of joy	0	joy lmao tears_of_joy	crying feels funny haha happy hehe hilarious laugh lol rofl roflmao tear
🙂	slightly smiling face	0	slightly_smiling_face	happy smile
🙃	upside-down face	0	upside_down_face	hehe smile upside-down
🫠	melting face	0	melting_face melt	disappear dissolve embarrassed haha heat hot liquid lol sarcasm sarcastic
😉	winking face	0	wink winking_face	flirt heartbreaker sexy slide tease winks
😊	smiling face with smiling eyes	0	blush smiling_face_with_closed_eyes	eye glad satisfied smile
😇	smiling face with halo	0	innocent halo	angel angelic angels blessed fairy fairytale fantasy happy peaceful smile spirit tale
🥰	smiling face with hearts	0	smiling_face_with_3_hearts smiling_face_with_three_hearts	adore crush heart ily love romance smile you
😍	smiling face with heart-eyes	0	heart_eyes smiling_face_with_heart_eyes	143 bae eye feels heart-eyes hearts ily kisses love romance romantic smile xoxo
🤩	star-struck	0	star-struck grinning_face_with_star_eyes star_struck	excited smile starry-eyed wow
😘	face blowing a kiss	0	kissing_heart blowing_a_kiss	adorbs bae flirt ily love lover miss muah romantic smooch xoxo you
😗	kissing face	0	kissing kissing_face	143 date dating flirt ily kiss love smooch smooches xoxo you
☺️	smiling face	0	relaxed smiling_face	happy outlined smile
😚	kissing face with closed eyes	0	kissing_closed_eyes kissing_face_with_closed_eyes	143 bae blush date dating eye flirt ily kisses smooches xoxo
😙	kissing face with smiling eyes	0	kissing_smiling_eyes kissing_face_with_smiling_eyes	143 closed date dating eye flirt ily kiss kisses love night smile
🥲	smiling face with tear	0	smiling_face_with_tear	glad grateful happy joy pain proud relieved smile smiley touched
😋	face savoring food	0	yum savoring_food	delicious eat full hungry savor smile smiling tasty um yummy
😛	face with tongue	0	stuck_out_tongue face_with_tongue	awesome cool nice party stuck-out sweet
😜	winking face with tongue	0	stuck_out_tongue_winking_eye	crazy epic funny joke loopy nutty party stuck-out wacky weirdo wink yolo
🤪	zany face	0	zany_face grinning_face_with_one_large_and_one_small_eye zany	crazy eyes goofy
😝	squinting face with tongue	0	stuck_out_tongue_closed_eyes	eye gross horrible omg stuck-out taste whatever yolo
🤑	money-mouth face	0	money_mouth_face	money-mouth paid
🤗	smiling face with open hands	0	hugging_face hugs hug hugging	
🤭	face with hand over mouth	0	face_with_hand_over_mouth smiling_face_with_smiling_eyes_and_hand_covering_mouth hand_over_mouth	giggle giggling oops realization secret shock sudden surprise whoops
🫢	face with open eyes and hand over mouth	0	face_with_open_eyes_and_hand_over_mouth face_with_open_eyes_hand_over_mouth gasp	amazement awe disbelief embarrass omg quiet scared shock surprise
🫣	face with peeking eye	0	face_with_peeking_eye peek	captivated embarrass hide hiding peep scared shy stare
🤫	shushing face	0	shushing_face face_with_finger_covering_closed_lips shush	quiet shh
🤔	thinking face	0	thinking_face thinking wtf	chin consider hmm ponder pondering wondering
🫡	saluting face	0	saluting_face salute	good luck ma’am ok respect sir troops yes
🤐	zipper-mouth face	0	zipper_mouth_face zipper_mouth	keep quiet secret shut zip zipper-mouth
🤨	face with raised eyebrow	0	face_with_raised_eyebrow face_with_one_eyebrow_raised raised_eyebrow	disapproval disbelief distrust emoji hmm mild skeptic skeptical skepticism surprise what
😐️	neutral face	0	neutral_face neutral	awkward blank deadpan expressionless fine jealous meh oh shade straight unamused unhappy unimpressed whatever
😑	expressionless face	0	expressionless expressionless_face	awkward dead fine inexpressive jealous meh not oh omg straight uh unhappy unimpressed whatever
😶	face without mouth	0	no_mouth	awkward blank expressionless mouthless mute quiet secret silence silent speechless
🫥	dotted line face	0	dotted_line_face	depressed disappear hidden hide introvert invisible meh whatever wtv
😶‍🌫️	face in clouds	0	face_in_clouds in_clouds	absentminded fog head
😏	smirking face	0	smirk smirking smirking_face	boss dapper flirt homie kidding leer shade slick sly smug snicker suave suspicious swag
😒	unamused face	0	unamused unamused_face	... bored fine jealous jel jelly pissed smh ugh uhh unhappy weird whatever
🙄	face with rolling eyes	0	face_with_rolling_eyes roll_eyes rolling_eyes	eyeroll shade ugh whatever
😬	grimacing face	0	grimacing grimacing_face	awk awkward dentist grimace grinning smile smiling
😮‍💨	face exhaling	0	face_exhaling exhale exhaling	blow blowing exhausted gasp groan relief sigh smiley smoke whisper whistle
🤥	lying face	0	lying_face lying	liar lie pinocchio
🫨	shaking face	0	shaking_face shaking	crazy daze earthquake omg panic shock surprise vibrate whoa wow
🙂‍↔️	head shaking horizontally	0	head_shaking_horizontally	no shake
🙂‍↕️	head shaking vertically	0	head_shaking_vertically	nod yes
😌	relieved face	0	relieved relieved_face	calm peace relief zen
😔	pensive face	0	pensive pensive_face	awful bored dejected died disappointed losing lost sad sucks
😪	sleepy face	0	sleepy sleepy_face	crying good night sad sleep sleeping tired
🤤	drooling face	0	drooling_face drooling	
😴	sleeping face	0	sleeping sleeping_face	bed bedtime good goodnight nap night sleep tired whatever yawn zzz
🫩	face with bags under eyes	0	face_with_bags_under_eyes face_with_eye_bags	bored exhausted fatigued late sleepy tired weary
😷	face with medical mask	0	mask medical_mask	cold dentist dermatologist doctor dr germs medicine sick
🤒	face with thermometer	0	face_with_thermometer	ill sick
🤕	face with head-bandage	0	face_with_head_bandage	head-bandage hurt injury ouch
🤢	nauseated face	0	nauseated_face nauseated	gross nasty sick vomit
🤮	face vomiting	0	face_vomiting face_with_open_mouth_vomiting vomiting_face vomiting	barf ew gross puke sick spew throw up vomit
🤧	sneezing face	0	sneezing_face sneezing	fever flu gesundheit sick sneeze
🥵	hot face	0	hot_face hot	dying feverish heat panting red-faced stroke sweating tongue
🥶	cold face	0	cold_face cold	blue blue-faced freezing frostbite icicles subzero teeth
🥴	woozy face	0	woozy_face woozy	dizzy drunk eyes intoxicated mouth tipsy uneven wavy
😵	face with crossed-out eyes	0	dizzy_face knocked_out	crossed-out dead feels sick tired
😵‍💫	face with spiral eyes	0	face_with_spiral_eyes dizzy_eyes	confused hypnotized omg smiley trouble whoa woah woozy
🤯	exploding head	0	exploding_head shocked_face_with_exploding_head	blown explode mind mindblown no way
🤠	cowboy hat face	0	face_with_cowboy_hat cowboy_hat_face cowboy cowboy_face	cowgirl
🥳	partying face	0	partying_face hooray partying	bday birthday celebrate celebration excited happy hat horn party
🥸	disguised face	0	disguised_face disguised	disguise eyebrow glasses incognito moustache mustache nose person spy tache tash
😎	smiling face with sunglasses	0	sunglasses smiling_face_with_sunglasses sunglasses_cool too_cool	awesome beach bright bro chilling rad relaxed shades slay smile style swag win
🤓	nerd face	0	nerd_face nerd	brainy clever expert geek gifted glasses intelligent smart
🧐	face with monocle	0	face_with_monocle monocle_face	classy fancy rich stuffy wealthy
😕	confused face	0	confused confused_face	befuddled confusing dunno frown hm meh not sad sorry sure
🫤	face with diagonal mouth	0	face_with_diagonal_mouth	confused confusion disappointed doubt doubtful frustrated frustration meh skeptical unsure whatever wtv
😟	worried face	0	worried worried_face	anxious butterflies nerves nervous sad stress stressed surprised worry
🙁	slightly frowning face	0	slightly_frowning_face	frown sad
☹️	frowning face	0	white_frowning_face frowning_face	frown sad
😮	face with open mouth	0	open_mouth face_with_open_mouth	believe forgot omg shocked surprised sympathy unbelievable unreal whoa wow you
😯	hushed face	0	hushed hushed_face	epic omg stunned surprised whoa woah
😲	astonished face	0	astonished astonished_face	cost no omg shocked totally way
😳	flushed face	0	flushed flushed_face	amazed awkward crazy dazed dead disbelief embarrassed geez heat hot impressed jeez what wow
🫪	distorted face	0	distorted_face	anxiety bloated panic shocked surprised vulnerable
🥺	pleading face	0	pleading_face pleading	begging big eyes mercy not please pretty puppy sad why
🥹	face holding back tears	0	face_holding_back_tears watery_eyes	admiration aww cry embarrassed feelings grateful gratitude joy please proud resist sad
😦	frowning face with open mouth	0	frowning	caught frown guard scared scary surprise what wow
😧	anguished face	0	anguished anguished_face	forgot scared scary stressed surprise unhappy what wow
😨	fearful face	0	fearful fearful_face	afraid anxious blame fear scared worried
😰	anxious face with sweat	0	cold_sweat anxious anxious_face	blue eek mouth nervous open rushed scared yikes
😥	sad but relieved face	0	disappointed_relieved sad_relieved_face	anxious call close complicated not sweat time whew
😢	crying face	0	cry crying_face	awful feels miss sad tear triste unhappy
😭	loudly crying face	0	sob loudly_crying_face	bawling cry sad tear tears unhappy
😱	face screaming in fear	0	scream screaming_in_fear	epic fearful munch scared screamer shocked surprised woah
😖	confounded face	0	confounded confounded_face	annoyed confused cringe distraught feels frustrated mad sad
😣	persevering face	0	persevere persevering_face	concentrate concentration focus headache
😞	disappointed face	0	disappointed disappointed_face	awful blame dejected fail losing sad unhappy
😓	downcast face with sweat	0	sweat downcast_face	close cold feels headache nervous sad scared yikes
😩	weary face	0	weary weary_face	crying fail feels hungry mad nooo sad sleepy tired unhappy
😫	tired face	0	tired_face tired	cost feels nap sad sneeze
🥱	yawning face	0	yawning_face yawn yawning	bedtime bored goodnight nap night sleep sleepy tired whatever zzz
😤	face with steam from nose	0	triumph nose_steam	anger angry feels fume fuming furious fury mad unhappy won
😡	enraged face	0	rage pout	anger angry feels mad maddening pouting red shade unhappy upset
😠	angry face	0	angry angry_face	anger blame feels frustrated mad maddening rage shade unhappy upset
🤬	face with symbols on mouth	0	face_with_symbols_on_mouth serious_face_with_symbols_covering_mouth cursing_face censored	censor cussing mad pissed swearing
😈	smiling face with horns	0	smiling_imp	demon devil evil fairy fairytale fantasy purple shade smile tale
👿	angry face with horns	0	imp angry_imp	demon devil evil fairy fairytale fantasy mischievous purple shade tale
💀	skull	0	skull	body dead death face fairy fairytale i’m lmao monster tale yolo
☠️	skull and crossbones	0	skull_and_crossbones	bone dead death face monster
💩	pile of poo	0	hankey poop shit	bs comic doo dung face fml monster smelly smh stink stinks stinky turd
🤡	clown face	0	clown_face clown	
👹	ogre	0	japanese_ogre ogre	creature devil face fairy fairytale fantasy mask monster scary tale
👺	goblin	0	japanese_goblin goblin	angry creature face fairy fairytale fantasy mask mean monster tale
👻	ghost	0	ghost	boo creature excited face fairy fairytale fantasy halloween haunting monster scary silly tale
👽️	alien	0	alien	creature extraterrestrial face fairy fairytale fantasy monster space tale ufo
👾	alien monster	0	space_invader alien_monster	creature extraterrestrial face fairy fairytale fantasy game gamer games pixelated tale ufo
🤖	robot	0	robot_face robot	monster
😺	grinning cat	0	smiley_cat grinning_cat	animal face mouth open smile smiling
😸	grinning cat with smiling eyes	0	smile_cat grinning_cat_with_closed_eyes	animal eye face grin
😹	cat with tears of joy	0	joy_cat tears_of_joy_cat	animal face laugh laughing lol tear
😻	smiling cat with heart-eyes	0	heart_eyes_cat smiling_cat_with_heart_eyes	animal eye face heart-eyes love smile
😼	cat with wry smile	0	smirk_cat wry_smile_cat	animal face ironic
😽	kissing cat	0	kissing_cat	animal closed eye eyes face kiss
🙀	weary cat	0	scream_cat weary_cat	animal face oh surprised
😿	crying cat	0	crying_cat_face crying_cat	animal cry sad tear
😾	pouting cat	0	pouting_cat	animal face
🙈	see-no-evil monkey	0	see_no_evil	embarrassed face forbidden forgot gesture hide omg prohibited scared secret smh watch
🙉	hear-no-evil monkey	0	hear_no_evil	animal ears face forbidden gesture listen not prohibited secret shh tmi
🙊	speak-no-evil monkey	0	speak_no_evil	animal face forbidden gesture not oops prohibited quiet secret stealth
💌	love letter	0	love_letter	heart mail romance valentine
💘	heart with arrow	0	cupid heart_with_arrow	143 adorbs date emotion ily love romance valentine
💝	heart with ribbon	0	gift_heart heart_with_ribbon	143 anniversary emotion ily kisses valentine xoxo
💖	sparkling heart	0	sparkling_heart	143 emotion excited good ily kisses morning night sparkle xoxo
💗	growing heart	0	heartpulse growing_heart	143 emotion excited ily kisses muah nervous pulse xoxo
💓	beating heart	0	heartbeat beating_heart	143 cardio emotion ily love pulsating pulse
💞	revolving hearts	0	revolving_hearts	143 adorbs anniversary emotion heart
💕	two hearts	0	two_hearts	143 anniversary date dating emotion heart ily kisses love loving xoxo
💟	heart decoration	0	heart_decoration	143 emotion hearth purple white
❣️	heart exclamation	0	heavy_heart_exclamation_mark_ornament heavy_heart_exclamation heart_exclamation	punctuation
💔	broken heart	0	broken_heart	break crushed emotion heartbroken lonely sad
❤️‍🔥	heart on fire	0	heart_on_fire	burn love lust sacred
❤️‍🩹	mending heart	0	mending_heart	healthier improving recovering recuperating well
❤️	red heart	0	heart red_heart	emotion love
🩷	pink heart	0	pink_heart	143 adorable cute emotion ily like love special sweet
🧡	orange heart	0	orange_heart	143
💛	yellow heart	0	yellow_heart	143 cardiac emotion ily love
💚	green heart	0	green_heart	143 emotion ily love romantic
💙	blue heart	0	blue_heart	143 emotion ily love romance
🩵	light blue heart	0	light_blue_heart	143 cute cyan emotion ily like love sky special teal
💜	purple heart	0	purple_heart	143 bestest emotion ily love
🤎	brown heart	0	brown_heart	143
🖤	black heart	0	black_heart	evil wicked
🩶	grey heart	0	grey_heart gray_heart	143 emotion ily love silver slate special
🤍	white heart	0	white_heart	143
💋	kiss mark	0	kiss	dating emotion heart kissing lips romance sexy
💯	hundred points	0	100	a+ agree clearly definitely faithful fleek full keep perfect point score true truth yup
💢	anger symbol	0	anger	angry comic mad upset
🫯	fight cloud	0	fight_cloud	argument brawl debate disagreement ruckus wrestle
💥	collision	0	boom collision	bomb collide comic explode
💫	dizzy	0	dizzy	comic shining shooting star stars
💦	sweat droplets	0	sweat_drops	comic drip droplet splashing squirt water wet work workout
💨	dashing away	0	dash dashing_away	cloud comic fart fast go gone gotta running smoke
🕳️	hole	0	hole	
💬	speech balloon	0	speech_balloon	bubble comic dialog message sms talk text typing
👁️‍🗨️	eye in speech bubble	0	eye-in-speech-bubble eye_speech_bubble eye_in_speech_bubble	balloon witness
🗨️	left speech bubble	0	left_speech_bubble	balloon dialog
🗯️	right anger bubble	0	right_anger_bubble	angry balloon mad
💭	thought balloon	0	thought_balloon	bubble cartoon cloud comic daydream decisions dream idea invent invention realize think thoughts wonder
💤	zzz	0	zzz	comic good goodnight night sleep sleeping sleepy tired
👋	waving hand	0	wave waving_hand	bye cya g2g greetings gtg hello hey hi later outtie ttfn ttyl yo you
🤚	raised back of hand	0	raised_back_of_hand	backhand
🖐️	hand with fingers splayed	0	raised_hand_with_fingers_splayed	finger stop
✋️	raised hand	0	hand raised_hand high_five	5 stop
🖖	vulcan salute	0	spock-hand vulcan_salute vulcan	finger hands
🫱	rightwards hand	0	rightwards_hand	handshake hold reach right rightward shake
🫲	leftwards hand	0	leftwards_hand	handshake hold left leftward reach shake
🫳	palm down hand	0	palm_down_hand palm_down	dismiss drop dropped pick shoo up
🫴	palm up hand	0	palm_up_hand palm_up	beckon catch come hold know lift me offer tell
🫷	leftwards pushing hand	0	leftwards_pushing_hand	block five halt high hold leftward pause push refuse slap stop wait
🫸	rightwards pushing hand	0	rightwards_pushing_hand	block five halt high hold pause push refuse rightward slap stop wait
👌	ok hand	0	ok_hand	awesome bet dope fleek fosho got gotcha legit okay pinch rad sure sweet three
🤌	pinched fingers	0	pinched_fingers pinch	gesture hand hold huh interrogation patience relax sarcastic ugh what zip
🤏	pinching hand	0	pinching_hand	amount bit fingers little small sort
✌️	victory hand	0	v victory	peace
🤞	crossed fingers	0	crossed_fingers hand_with_index_and_middle_fingers_crossed fingers_crossed	cross finger luck
🫰	hand with index finger and thumb crossed	0	hand_with_index_finger_and_thumb_crossed	<3 expensive heart love money snap
🤟	love-you gesture	0	i_love_you_hand_sign love_you_gesture	fingers ily love-you three
🤘	sign of the horns	0	the_horns sign_of_the_horns metal	finger hand rock-on
🤙	call me hand	0	call_me_hand	hang loose shaka
👈️	backhand index pointing left	0	point_left	finger hand
👉️	backhand index pointing right	0	point_right	finger hand
👆️	backhand index pointing up	0	point_up_2	finger hand
🖕	middle finger	0	middle_finger reversed_hand_with_middle_finger_extended fu	
👇️	backhand index pointing down	0	point_down	finger hand
☝️	index pointing up	0	point_up	finger hand this
🫵	index pointing at the viewer	0	index_pointing_at_the_viewer point_forward	finger hand poke you
👍️	thumbs up	0	+1 thumbsup yes	good hand like thumb
👎️	thumbs down	0	-1 thumbsdown no	-1 bad dislike good hand nope thumb
✊️	raised fist	0	fist fist_raised	clenched hand punch solidarity
👊	oncoming fist	0	facepunch punch fist_oncoming	absolutely agree boom bro bruh bump clenched correct hand knuckle pound rock ttyl
🤛	left-facing fist	0	left-facing_fist fist_left left_facing_fist	left-facing leftwards
🤜	right-facing fist	0	right-facing_fist fist_right right_facing_fist	right-facing rightwards
👏	clapping hands	0	clap clapping_hands	applause approval awesome congrats congratulations excited good great hand homie job nice prayed well yay
🙌	raising hands	0	raised_hands	celebration gesture hand hooray praise
🫶	heart hands	0	heart_hands	<3 love you
👐	open hands	0	open_hands	hand hug jazz swerve
🤲	palms up together	0	palms_up_together	cupped dua hands pray prayer wish
🤝	handshake	0	handshake	agreement deal hand meeting shake
🙏	folded hands	0	pray folded_hands	appreciate ask beg blessed bow cmon five gesture hand high please thanks thx
✍️	writing hand	0	writing_hand	write
💅	nail polish	0	nail_care nail_polish	bored cosmetics done makeup manicure whatever
🤳	selfie	0	selfie	camera phone
💪	flexed biceps	0	muscle right_bicep	arm beast bench bodybuilder bro curls flex gains gym jacked press ripped strong weightlift
🦾	mechanical arm	0	mechanical_arm	accessibility prosthetic
🦿	mechanical leg	0	mechanical_leg	accessibility prosthetic
🦵	leg	0	leg	bent foot kick knee limb
🦶	foot	0	foot	ankle feet kick stomp
👂️	ear	0	ear	body ears hear hearing listen listening sound
🦻	ear with hearing aid	0	ear_with_hearing_aid hearing_aid	accessibility hard
👃	nose	0	nose	body noses nosey odor smell smells
🧠	brain	0	brain	intelligent smart
🫀	anatomical heart	0	anatomical_heart	beat cardiology heartbeat organ pulse real red
🫁	lungs	0	lungs	breath breathe exhalation inhalation lung organ respiration
🦷	tooth	0	tooth	dentist pearly teeth white
🦴	bone	0	bone	bones dog skeleton wishbone
👀	eyes	0	eyes	body eye face googly look looking omg peep see seeing
👁️	eye	0	eye	1 body one
👅	tongue	0	tongue	body lick slurp
👄	mouth	0	lips mouth	beauty body kiss kissing lipstick
🫦	biting lip	0	biting_lip	anxious bite fear flirt flirting kiss lipstick nervous sexy uncomfortable worried worry
👶	baby	0	baby	babies children goo infant newborn pregnant young
🧒	child	0	child	bright-eyed grandchild kid young younger
👦	boy	0	boy	bright-eyed child grandson kid son young younger
👧	girl	0	girl	bright-eyed child daughter granddaughter kid virgo young younger zodiac
🧑	person	0	adult	
👱	person: blond hair	0	person_with_blond_hair blond_haired_person blond_haired	blond-haired human
👨	man	0	man	adult bro
🧔	person: beard	0	bearded_person person_bearded	whiskers
🧔‍♂️	man: beard	0	man_with_beard man_beard man_bearded	whiskers
🧔‍♀️	woman: beard	0	woman_with_beard woman_beard woman_bearded	whiskers
👨‍🦰	man: red hair	0	red_haired_man man_red_haired	adult bro
👨‍🦱	man: curly hair	0	curly_haired_man man_curly_haired	adult bro
👨‍🦳	man: white hair	0	white_haired_man man_white_haired	adult bro
👨‍🦲	man: bald	0	bald_man man_bald	adult bro
👩	woman	0	woman	adult lady
👩‍🦰	woman: red hair	0	red_haired_woman woman_red_haired	adult lady
🧑‍🦰	person: red hair	0	red_haired_person person_red_hair red_haired	adult
👩‍🦱	woman: curly hair	0	curly_haired_woman woman_curly_haired	adult lady
🧑‍🦱	person: curly hair	0	curly_haired_person person_curly_hair curly_haired	adult
👩‍🦳	woman: white hair	0	white_haired_woman woman_white_haired	adult lady
🧑‍🦳	person: white hair	0	white_haired_person person_white_hair white_haired	adult
👩‍🦲	woman: bald	0	bald_woman woman_bald	adult lady
🧑‍🦲	person: bald	0	bald_person person_bald bald	adult
👱‍♀️	woman: blond hair	0	blond-haired-woman blond_haired_woman blonde_woman woman_blond_haired	blond-haired
👱‍♂️	man: blond hair	0	blond-haired-man blond_haired_man man_blond_haired	blond-haired
🧓	older person	0	older_adult	elderly grandparent old wise
👴	old man	0	older_man	adult bald elderly gramps grandfather grandpa wise
👵	old woman	0	older_woman	adult elderly grandma grandmother granny lady wise
🙍	person frowning	0	person_frowning frowning_person	annoyed disappointed disgruntled disturbed frown frustrated gesture irritated upset
🙍‍♂️	man frowning	0	man-frowning frowning_man man_frowning	annoyed disappointed disgruntled disturbed frown frustrated gesture irritated upset
🙍‍♀️	woman frowning	0	woman-frowning frowning_woman woman_frowning	annoyed disappointed disgruntled disturbed frown frustrated gesture irritated upset
🙎	person pouting	0	person_with_pouting_face pouting_face person_pouting pouting	disappointed downtrodden frown grimace scowl sulk upset whine
🙎‍♂️	man pouting	0	man-pouting pouting_man man_pouting	disappointed downtrodden frown grimace scowl sulk upset whine
🙎‍♀️	woman pouting	0	woman-pouting pouting_woman woman_pouting	disappointed downtrodden frown grimace scowl sulk upset whine
🙅	person gesturing no	0	no_good person_gesturing_no	forbidden gesture hand not prohibit
🙅‍♂️	man gesturing no	0	man-gesturing-no ng_man no_good_man man_gesturing_no	forbidden gesture hand not prohibit
🙅‍♀️	woman gesturing no	0	woman-gesturing-no ng_woman no_good_woman woman_gesturing_no	forbidden gesture hand not prohibit
🙆	person gesturing ok	0	ok_woman ok_person all_good person_gesturing_ok	exercise gesture hand omg
🙆‍♂️	man gesturing ok	0	man-gesturing-ok ok_man man_gesturing_ok	exercise gesture hand omg
🙆‍♀️	woman gesturing ok	0	woman-gesturing-ok woman_gesturing_ok	exercise gesture hand omg
💁	person tipping hand	0	information_desk_person tipping_hand_person person_tipping_hand	fetch flick flip gossip sarcasm sarcastic sassy seriously whatever
💁‍♂️	man tipping hand	0	man-tipping-hand sassy_man tipping_hand_man man_tipping_hand	fetch flick flip gossip sarcasm sarcastic seriously whatever
💁‍♀️	woman tipping hand	0	woman-tipping-hand sassy_woman tipping_hand_woman woman_tipping_hand	fetch flick flip gossip sarcasm sarcastic seriously whatever
🙋	person raising hand	0	raising_hand person_raising_hand	gesture here know me pick question raise
🙋‍♂️	man raising hand	0	man-raising-hand raising_hand_man man_raising_hand	gesture here know me pick question raise
🙋‍♀️	woman raising hand	0	woman-raising-hand raising_hand_woman woman_raising_hand	gesture here know me pick question raise
🧏	deaf person	0	deaf_person	accessibility ear gesture hear
🧏‍♂️	deaf man	0	deaf_man	accessibility ear gesture hear
🧏‍♀️	deaf woman	0	deaf_woman	accessibility ear gesture hear
🙇	person bowing	0	bow person_bowing	apology ask beg favor forgive gesture meditate meditation pity regret sorry
🙇‍♂️	man bowing	0	man-bowing bowing_man man_bowing	apology ask beg bow favor forgive gesture meditate meditation pity regret sorry
🙇‍♀️	woman bowing	0	woman-bowing bowing_woman woman_bowing	apology ask beg bow favor forgive gesture meditate meditation pity regret sorry
🤦	person facepalming	0	face_palm facepalm person_facepalming	again bewilder disbelief exasperation no not oh omg shock smh
🤦‍♂️	man facepalming	0	man-facepalming man_facepalming	again bewilder disbelief exasperation facepalm no not oh omg shock smh
🤦‍♀️	woman facepalming	0	woman-facepalming woman_facepalming	again bewilder disbelief exasperation facepalm no not oh omg shock smh
🤷	person shrugging	0	shrug person_shrugging	doubt dunno guess idk ignorance indifference knows maybe whatever who
🤷‍♂️	man shrugging	0	man-shrugging man_shrugging	doubt dunno guess idk ignorance indifference knows maybe shrug whatever who
🤷‍♀️	woman shrugging	0	woman-shrugging woman_shrugging	doubt dunno guess idk ignorance indifference knows maybe shrug whatever who
🧑‍⚕️	health worker	0	health_worker	doctor healthcare nurse therapist
👨‍⚕️	man health worker	0	male-doctor man_health_worker	healthcare nurse therapist
👩‍⚕️	woman health worker	0	female-doctor woman_health_worker	healthcare nurse therapist
🧑‍🎓	student	0	student	graduate
👨‍🎓	man student	0	male-student man_student	graduate
👩‍🎓	woman student	0	female-student woman_student	graduate
🧑‍🏫	teacher	0	teacher	instructor lecturer professor
👨‍🏫	man teacher	0	male-teacher man_teacher	instructor lecturer professor
👩‍🏫	woman teacher	0	female-teacher woman_teacher	instructor lecturer professor
🧑‍⚖️	judge	0	judge	justice law scales
👨‍⚖️	man judge	0	male-judge man_judge	justice law scales
👩‍⚖️	woman judge	0	female-judge woman_judge	justice law scales
🧑‍🌾	farmer	0	farmer	gardener rancher
👨‍🌾	man farmer	0	male-farmer man_farmer	gardener rancher
👩‍🌾	woman farmer	0	female-farmer woman_farmer	gardener rancher
🧑‍🍳	cook	0	cook	chef
👨‍🍳	man cook	0	male-cook man_cook	chef
👩‍🍳	woman cook	0	female-cook woman_cook	chef
🧑‍🔧	mechanic	0	mechanic	electrician plumber tradesperson
👨‍🔧	man mechanic	0	male-mechanic man_mechanic	electrician plumber tradesperson
👩‍🔧	woman mechanic	0	female-mechanic woman_mechanic	electrician plumber tradesperson
🧑‍🏭	factory worker	0	factory_worker	assembly industrial
👨‍🏭	man factory worker	0	male-factory-worker man_factory_worker	assembly industrial
👩‍🏭	woman factory worker	0	female-factory-worker woman_factory_worker	assembly industrial
🧑‍💼	office worker	0	office_worker	architect business manager white-collar
👨‍💼	man office worker	0	male-office-worker man_office_worker	architect business manager white-collar
👩‍💼	woman office worker	0	female-office-worker woman_office_worker	architect business manager white-collar
🧑‍🔬	scientist	0	scientist	biologist chemist engineer mathematician physicist
👨‍🔬	man scientist	0	male-scientist man_scientist	biologist chemist engineer mathematician physicist
👩‍🔬	woman scientist	0	female-scientist woman_scientist	biologist chemist engineer mathematician physicist
🧑‍💻	technologist	0	technologist	coder computer developer inventor software
👨‍💻	man technologist	0	male-technologist man_technologist	coder computer developer inventor software
👩‍💻	woman technologist	0	female-technologist woman_technologist	coder computer developer inventor software
🧑‍🎤	singer	0	singer	actor entertainer rock rockstar star
👨‍🎤	man singer	0	male-singer man_singer	actor entertainer rock rockstar star
👩‍🎤	woman singer	0	female-singer woman_singer	actor entertainer rock rockstar star
🧑‍🎨	artist	0	artist	palette
👨‍🎨	man artist	0	male-artist man_artist	palette
👩‍🎨	woman artist	0	female-artist woman_artist	palette
🧑‍✈️	pilot	0	pilot	plane
👨‍✈️	man pilot	0	male-pilot man_pilot	plane
👩‍✈️	woman pilot	0	female-pilot woman_pilot	plane
🧑‍🚀	astronaut	0	astronaut	rocket space
👨‍🚀	man astronaut	0	male-astronaut man_astronaut	rocket space
👩‍🚀	woman astronaut	0	female-astronaut woman_astronaut	rocket space
🧑‍🚒	firefighter	0	firefighter	fire firetruck
👨‍🚒	man firefighter	0	male-firefighter man_firefighter	fire firetruck
👩‍🚒	woman firefighter	0	female-firefighter woman_firefighter	fire firetruck
👮	police officer	0	cop police_officer	apprehend arrest citation law over pulled undercover
👮‍♂️	man police officer	0	male-police-officer policeman man_police_officer	apprehend arrest citation cop law over pulled undercover
👮‍♀️	woman police officer	0	female-police-officer policewoman woman_police_officer	apprehend arrest citation cop law over pulled undercover
🕵️	detective	0	sleuth_or_spy detective	
🕵️‍♂️	man detective	0	male-detective male_detective man_detective	sleuth spy
🕵️‍♀️	woman detective	0	female-detective female_detective woman_detective	sleuth spy
💂	guard	0	guardsman guard	buckingham helmet london palace
💂‍♂️	man guard	0	male-guard man_guard	buckingham helmet london palace
💂‍♀️	woman guard	0	female-guard guardswoman woman_guard	buckingham helmet london palace
🥷	ninja	0	ninja	assassin fight fighter hidden person secret skills sly soldier stealth war
👷	construction worker	0	construction_worker	build fix hardhat hat man person rebuild remodel repair work
👷‍♂️	man construction worker	0	male-construction-worker construction_worker_man man_construction_worker	build fix hardhat hat rebuild remodel repair work
👷‍♀️	woman construction worker	0	female-construction-worker construction_worker_woman woman_construction_worker	build fix hardhat hat man rebuild remodel repair work
🫅	person with crown	0	person_with_crown royalty	monarch noble regal royal
🤴	prince	0	prince	crown fairy fairytale fantasy king royal royalty tale
👸	princess	0	princess	crown fairy fairytale fantasy queen royal royalty tale
👳	person wearing turban	0	man_with_turban person_with_turban person_wearing_turban	
👳‍♂️	man wearing turban	0	man-wearing-turban man_wearing_turban	
👳‍♀️	woman wearing turban	0	woman-wearing-turban woman_with_turban woman_wearing_turban	
👲	person with skullcap	0	man_with_gua_pi_mao person_with_skullcap	cap chinese guapi hat
🧕	woman with headscarf	0	person_with_headscarf woman_with_headscarf	bandana head hijab kerchief mantilla tichel
🤵	person in tuxedo	0	person_in_tuxedo	formal wedding
🤵‍♂️	man in tuxedo	0	man_in_tuxedo	formal groom wedding
🤵‍♀️	woman in tuxedo	0	woman_in_tuxedo	formal wedding
👰	person with veil	0	bride_with_veil person_with_veil	wedding
👰‍♂️	man with veil	0	man_with_veil	wedding
👰‍♀️	woman with veil	0	woman_with_veil	bride wedding
🤰	pregnant woman	0	pregnant_woman	
🫃	pregnant man	0	pregnant_man	belly bloated full overeat
🫄	pregnant person	0	pregnant_person	belly bloated full overeat stuffed
🤱	breast-feeding	0	breast-feeding breast_feeding	baby mom mother nursing woman
👩‍🍼	woman feeding baby	0	woman_feeding_baby	feed mom mother nanny newborn nursing
👨‍🍼	man feeding baby	0	man_feeding_baby	dad father feed nanny newborn nursing
🧑‍🍼	person feeding baby	0	person_feeding_baby	feed nanny newborn nursing parent
👼	baby angel	0	angel	church face fairy fairytale fantasy tale
🎅	santa claus	0	santa	celebration christmas fairy fantasy father holiday merry tale xmas
🤶	mrs. claus	0	mrs_claus mother_christmas	celebration fairy fantasy holiday merry santa tale xmas
🧑‍🎄	mx claus	0	mx_claus	celebration christmas fairy fantasy holiday merry santa tale xmas
🦸	superhero	0	superhero	good hero superpower
🦸‍♂️	man superhero	0	male_superhero superhero_man man_superhero	good hero superpower
🦸‍♀️	woman superhero	0	female_superhero superhero_woman woman_superhero	good hero heroine superpower
🦹	supervillain	0	supervillain	bad criminal evil superpower villain
🦹‍♂️	man supervillain	0	male_supervillain supervillain_man man_supervillain	bad criminal evil superpower villain
🦹‍♀️	woman supervillain	0	female_supervillain supervillain_woman woman_supervillain	bad criminal evil superpower villain
🧙	mage	0	mage	fantasy magic play sorcerer sorceress sorcery spell summon witch wizard
🧙‍♂️	man mage	0	male_mage mage_man man_mage	fantasy magic play sorcerer sorceress sorcery spell summon witch wizard
🧙‍♀️	woman mage	0	female_mage mage_woman woman_mage	fantasy magic play sorcerer sorceress sorcery spell summon witch wizard
🧚	fairy	0	fairy	fairytale fantasy myth person pixie tale wings
🧚‍♂️	man fairy	0	male_fairy fairy_man man_fairy	fairytale fantasy myth oberon person pixie puck tale wings
🧚‍♀️	woman fairy	0	female_fairy fairy_woman woman_fairy	fairytale fantasy myth person pixie tale titania wings
🧛	vampire	0	vampire	blood dracula fangs halloween scary supernatural teeth undead
🧛‍♂️	man vampire	0	male_vampire vampire_man man_vampire	blood fangs halloween scary supernatural teeth undead
🧛‍♀️	woman vampire	0	female_vampire vampire_woman woman_vampire	blood fangs halloween scary supernatural teeth undead
🧜	merperson	0	merperson	creature fairytale folklore ocean sea siren trident
🧜‍♂️	merman	0	merman	creature fairytale folklore neptune ocean poseidon sea siren trident triton
🧜‍♀️	mermaid	0	mermaid	creature fairytale folklore merwoman ocean sea siren trident
🧝	elf	0	elf	elves enchantment fantasy folklore magic magical myth
🧝‍♂️	man elf	0	male_elf elf_man man_elf	elves enchantment fantasy folklore magic magical myth
🧝‍♀️	woman elf	0	female_elf elf_woman woman_elf	elves enchantment fantasy folklore magic magical myth
🧞	genie	0	genie	djinn fantasy jinn lamp myth rub wishes
🧞‍♂️	man genie	0	male_genie genie_man man_genie	djinn fantasy jinn lamp myth rub wishes
🧞‍♀️	woman genie	0	female_genie genie_woman woman_genie	djinn fantasy jinn lamp myth rub wishes
🧟	zombie	0	zombie	apocalypse dead halloween horror scary undead walking
🧟‍♂️	man zombie	0	male_zombie zombie_man man_zombie	apocalypse dead halloween horror scary undead walking
🧟‍♀️	woman zombie	0	female_zombie zombie_woman woman_zombie	apocalypse dead halloween horror scary undead walking
🧌	troll	0	troll	fairy fantasy monster tale trolling
🫈	hairy creature	0	hairy_creature	bigfoot cryptid forest giant sasquatch woodwose yeti
💆	person getting massage	0	massage person_getting_massage	face headache relax relaxing salon soothe spa tension therapy treatment
💆‍♂️	man getting massage	0	man-getting-massage massage_man man_getting_massage	face headache relax relaxing salon soothe spa tension therapy treatment
💆‍♀️	woman getting massage	0	woman-getting-massage massage_woman woman_getting_massage	face headache relax relaxing salon soothe spa tension therapy treatment
💇	person getting haircut	0	haircut person_getting_haircut	barber beauty chop cosmetology cut groom hair parlor shears style
💇‍♂️	man getting haircut	0	man-getting-haircut haircut_man man_getting_haircut	barber beauty chop cosmetology cut groom hair parlor person shears style
💇‍♀️	woman getting haircut	0	woman-getting-haircut haircut_woman woman_getting_haircut	barber beauty chop cosmetology cut groom hair parlor person shears style
🚶	person walking	0	walking person_walking	amble gait hike man pace pedestrian stride stroll walk
🚶‍♂️	man walking	0	man-walking walking_man man_walking	amble gait hike pace pedestrian stride stroll walk
🚶‍♀️	woman walking	0	woman-walking walking_woman woman_walking	amble gait hike man pace pedestrian stride stroll walk
🚶‍➡️	person walking: facing right	0	person_walking_facing_right person_walking_right	amble gait hike man pace pedestrian stride stroll walk
🚶‍♀️‍➡️	woman walking: facing right	0	woman_walking_facing_right woman_walking_right	amble gait hike man pace pedestrian stride stroll walk
🚶‍♂️‍➡️	man walking: facing right	0	man_walking_facing_right man_walking_right	amble gait hike pace pedestrian stride stroll walk
🧍	person standing	0	standing_person person_standing standing	stand
🧍‍♂️	man standing	0	man_standing standing_man	stand
🧍‍♀️	woman standing	0	woman_standing standing_woman	stand
🧎	person kneeling	0	kneeling_person kneeling person_kneeling	kneel knees
🧎‍♂️	man kneeling	0	man_kneeling kneeling_man	kneel knees
🧎‍♀️	woman kneeling	0	woman_kneeling kneeling_woman	kneel knees
🧎‍➡️	person kneeling: facing right	0	person_kneeling_facing_right person_kneeling_right	kneel knees
🧎‍♀️‍➡️	woman kneeling: facing right	0	woman_kneeling_facing_right woman_kneeling_right	kneel knees
🧎‍♂️‍➡️	man kneeling: facing right	0	man_kneeling_facing_right man_kneeling_right	kneel knees
🧑‍🦯	person with white cane	0	person_with_probing_cane person_with_white_cane	accessibility blind
🧑‍🦯‍➡️	person with white cane: facing right	0	person_with_white_cane_facing_right person_with_white_cane_right	accessibility blind probing
👨‍🦯	man with white cane	0	man_with_probing_cane man_with_white_cane	accessibility blind
👨‍🦯‍➡️	man with white cane: facing right	0	man_with_white_cane_facing_right man_with_white_cane_right	accessibility blind probing
👩‍🦯	woman with white cane	0	woman_with_probing_cane woman_with_white_cane	accessibility blind
👩‍🦯‍➡️	woman with white cane: facing right	0	woman_with_white_cane_facing_right woman_with_white_cane_right	accessibility blind probing
🧑‍🦼	person in motorized wheelchair	0	person_in_motorized_wheelchair	accessibility
🧑‍🦼‍➡️	person in motorized wheelchair: facing right	0	person_in_motorized_wheelchair_facing_right person_in_motorized_wheelchair_right	accessibility
👨‍🦼	man in motorized wheelchair	0	man_in_motorized_wheelchair	accessibility
👨‍🦼‍➡️	man in motorized wheelchair: facing right	0	man_in_motorized_wheelchair_facing_right man_in_motorized_wheelchair_right	accessibility
👩‍🦼	woman in motorized wheelchair	0	woman_in_motorized_wheelchair	accessibility
👩‍🦼‍➡️	woman in motorized wheelchair: facing right	0	woman_in_motorized_wheelchair_facing_right woman_in_motorized_wheelchair_right	accessibility
🧑‍🦽	person in manual wheelchair	0	person_in_manual_wheelchair	accessibility
🧑‍🦽‍➡️	person in manual wheelchair: facing right	0	person_in_manual_wheelchair_facing_right person_in_manual_wheelchair_right	accessibility
👨‍🦽	man in manual wheelchair	0	man_in_manual_wheelchair	accessibility
👨‍🦽‍➡️	man in manual wheelchair: facing right	0	man_in_manual_wheelchair_facing_right man_in_manual_wheelchair_right	accessibility
👩‍🦽	woman in manual wheelchair	0	woman_in_manual_wheelchair	accessibility
👩‍🦽‍➡️	woman in manual wheelchair: facing right	0	woman_in_manual_wheelchair_facing_right woman_in_manual_wheelchair_right	accessibility
🏃	person running	0	runner running person_running	fast hurry marathon move quick race racing run rush speed
🏃‍♂️	man running	0	man-running running_man man_running	fast hurry marathon move quick race racing run rush speed
🏃‍♀️	woman running	0	woman-running running_woman woman_running	fast hurry marathon move quick race racing run rush speed
🏃‍➡️	person running: facing right	0	person_running_facing_right person_running_right	fast hurry marathon move quick race racing run rush speed
🏃‍♀️‍➡️	woman running: facing right	0	woman_running_facing_right woman_running_right	fast hurry marathon move quick race racing run rush speed
🏃‍♂️‍➡️	man running: facing right	0	man_running_facing_right man_running_right	fast hurry marathon move quick race racing run rush speed
🧑‍🩰	ballet dancer	0	ballet_dancer	
💃	woman dancing	0	dancer woman_dancing	dance elegant festive flair flamenco groove let’s salsa tango
🕺	man dancing	0	man_dancing	dance dancer elegant festive flair flamenco groove let’s salsa tango
🕴️	person in suit levitating	0	man_in_business_suit_levitating business_suit_levitating levitate levitating person_in_suit_levitating	
👯	people with bunny ears	0	dancers people_with_bunny_ears_partying	bestie bff counterpart dancer double ear identical pair party soulmate twin twinsies
👯‍♂️	men with bunny ears	0	men-with-bunny-ears-partying man-with-bunny-ears-partying dancing_men men_with_bunny_ears_partying	bestie bff counterpart dancer double ear identical pair party people soulmate twin twinsies
👯‍♀️	women with bunny ears	0	women-with-bunny-ears-partying woman-with-bunny-ears-partying dancing_women women_with_bunny_ears_partying	bestie bff counterpart dancer double ear identical pair party people soulmate twin twinsies
🧖	person in steamy room	0	person_in_steamy_room sauna_person	day luxurious pamper relax spa steam steambath unwind
🧖‍♂️	man in steamy room	0	man_in_steamy_room sauna_man	day luxurious pamper relax spa steam steambath unwind
🧖‍♀️	woman in steamy room	0	woman_in_steamy_room sauna_woman	day luxurious pamper relax spa steam steambath unwind
🧗	person climbing	0	person_climbing climbing	climb climber mountain rock scale up
🧗‍♂️	man climbing	0	man_climbing climbing_man	climb climber mountain rock scale up
🧗‍♀️	woman climbing	0	woman_climbing climbing_woman	climb climber mountain rock scale up
🤺	person fencing	0	fencer person_fencing fencing	sword
🏇	horse racing	0	horse_racing	jockey racehorse riding sport
⛷️	skier	0	skier person_skiing skiing	ski snow
🏂️	snowboarder	0	snowboarder person_snowboarding snowboarding	ski snow snowboard sport
🏌️	person golfing	0	golfer golfing person_golfing	ball birdie caddy driving golf green pga putt range tee
🏌️‍♂️	man golfing	0	man-golfing golfing_man man_golfing	ball birdie caddy driving golf green pga putt range tee
🏌️‍♀️	woman golfing	0	woman-golfing golfing_woman woman_golfing	ball birdie caddy driving golf green pga putt range tee
🏄️	person surfing	0	surfer person_surfing surfing	beach ocean sport surf swell waves
🏄‍♂️	man surfing	0	man-surfing surfing_man man_surfing	beach ocean sport surf surfer swell waves
🏄‍♀️	woman surfing	0	woman-surfing surfing_woman woman_surfing	beach ocean person sport surf surfer swell waves
🚣	person rowing boat	0	rowboat person_rowing_boat	canoe cruise fishing lake oar paddle raft river row
🚣‍♂️	man rowing boat	0	man-rowing-boat rowing_man man_rowing_boat	canoe cruise fishing lake oar paddle raft river row rowboat
🚣‍♀️	woman rowing boat	0	woman-rowing-boat rowing_woman woman_rowing_boat	canoe cruise fishing lake oar paddle raft river row rowboat
🏊️	person swimming	0	swimmer person_swimming swimming	freestyle sport swim triathlon
🏊‍♂️	man swimming	0	man-swimming swimming_man man_swimming	freestyle sport swim swimmer triathlon
🏊‍♀️	woman swimming	0	woman-swimming swimming_woman woman_swimming	freestyle man sport swim swimmer triathlon
⛹️	person bouncing ball	0	person_with_ball bouncing_ball_person person_bouncing_ball	athletic basketball championship dribble net player throw
⛹️‍♂️	man bouncing ball	0	man-bouncing-ball basketball_man bouncing_ball_man man_bouncing_ball	athletic championship dribble net player throw
⛹️‍♀️	woman bouncing ball	0	woman-bouncing-ball basketball_woman bouncing_ball_woman woman_bouncing_ball	athletic championship dribble net player throw
🏋️	person lifting weights	0	weight_lifter weight_lifting person_lifting_weights	barbell bodybuilder deadlift powerlifting weightlifter workout
🏋️‍♂️	man lifting weights	0	man-lifting-weights weight_lifting_man man_lifting_weights	barbell bodybuilder deadlift lifter powerlifting weightlifter workout
🏋️‍♀️	woman lifting weights	0	woman-lifting-weights weight_lifting_woman woman_lifting_weights	barbell bodybuilder deadlift lifter powerlifting weightlifter workout
🚴	person biking	0	bicyclist biking person_biking	bicycle bike cycle cyclist riding sport
🚴‍♂️	man biking	0	man-biking biking_man man_biking	bicycle bicyclist bike cycle cyclist riding sport
🚴‍♀️	woman biking	0	woman-biking biking_woman woman_biking	bicycle bicyclist bike cycle cyclist riding sport
🚵	person mountain biking	0	mountain_bicyclist mountain_biking person_mountain_biking	bicycle bike cycle cyclist riding sport
🚵‍♂️	man mountain biking	0	man-mountain-biking mountain_biking_man man_mountain_biking	bicycle bicyclist bike cycle cyclist riding sport
🚵‍♀️	woman mountain biking	0	woman-mountain-biking mountain_biking_woman woman_mountain_biking	bicycle bicyclist bike cycle cyclist riding sport
🤸	person cartwheeling	0	person_doing_cartwheel cartwheeling person_cartwheel	active excited flip gymnastics happy somersault
🤸‍♂️	man cartwheeling	0	man-cartwheeling man_cartwheeling	active cartwheel excited flip gymnastics happy somersault
🤸‍♀️	woman cartwheeling	0	woman-cartwheeling woman_cartwheeling	active cartwheel excited flip gymnastics happy somersault
🤼	people wrestling	0	wrestlers wrestling people_wrestling	combat duel grapple ring tournament wrestle
🤼‍♂️	men wrestling	0	man-wrestling men_wrestling	combat duel grapple ring tournament wrestle
🤼‍♀️	women wrestling	0	woman-wrestling women_wrestling	combat duel grapple ring tournament wrestle
🤽	person playing water polo	0	water_polo person_playing_water_polo	sport swimming waterpolo
🤽‍♂️	man playing water polo	0	man-playing-water-polo man_playing_water_polo	sport swimming waterpolo
🤽‍♀️	woman playing water polo	0	woman-playing-water-polo woman_playing_water_polo	sport swimming waterpolo
🤾	person playing handball	0	handball handball_person person_playing_handball	athletics ball catch chuck hurl lob pitch sport throw toss
🤾‍♂️	man playing handball	0	man-playing-handball man_playing_handball	athletics ball catch chuck hurl lob pitch sport throw toss
🤾‍♀️	woman playing handball	0	woman-playing-handball woman_playing_handball	athletics ball catch chuck hurl lob pitch sport throw toss
🤹	person juggling	0	juggling juggling_person juggler person_juggling	act balance balancing handle juggle manage multitask skill
🤹‍♂️	man juggling	0	man-juggling man_juggling	act balance balancing handle juggle manage multitask skill
🤹‍♀️	woman juggling	0	woman-juggling woman_juggling	act balance balancing handle juggle manage multitask skill
🧘	person in lotus position	0	person_in_lotus_position lotus_position	cross legged legs meditation peace relax serenity yoga yogi zen
🧘‍♂️	man in lotus position	0	man_in_lotus_position lotus_position_man	cross legged legs meditation peace relax serenity yoga yogi zen
🧘‍♀️	woman in lotus position	0	woman_in_lotus_position lotus_position_woman	cross legged legs meditation peace relax serenity yoga yogi zen
🛀	person taking bath	0	bath person_taking_bath	bathtub tub
🛌	person in bed	0	sleeping_accommodation sleeping_bed person_in_bed	bedtime good goodnight hotel nap night sleep tired zzz
🧑‍🤝‍🧑	people holding hands	0	people_holding_hands	bae bestie bff couple dating flirt friends hand hold twins
👭	women holding hands	0	two_women_holding_hands women_holding_hands	bae bestie bff couple dating flirt friends girls hand hold sisters twins
👫	woman and man holding hands	0	man_and_woman_holding_hands woman_and_man_holding_hands couple	bae bestie bff dating flirt friends hand hold twins
👬	men holding hands	0	two_men_holding_hands men_holding_hands	bae bestie bff boys brothers couple dating flirt friends hand hold twins
💏	kiss	0	couplekiss couple_kiss	anniversary babe bae date dating heart love mwah person romance together xoxo
👩‍❤️‍💋‍👨	kiss: woman, man	0	woman-kiss-man couplekiss_man_woman kiss_mw kiss_wm	anniversary babe bae couple date dating heart love mwah person romance together xoxo
👨‍❤️‍💋‍👨	kiss: man, man	0	man-kiss-man couplekiss_man_man kiss_mm	anniversary babe bae couple date dating heart love mwah person romance together xoxo
👩‍❤️‍💋‍👩	kiss: woman, woman	0	woman-kiss-woman couplekiss_woman_woman kiss_ww	anniversary babe bae couple date dating heart love mwah person romance together xoxo
💑	couple with heart	0	couple_with_heart	anniversary babe bae dating kiss love person relationship romance together you
👩‍❤️‍👨	couple with heart: woman, man	0	woman-heart-man couple_with_heart_woman_man couple_with_heart_mw couple_with_heart_wm	anniversary babe bae dating kiss love person relationship romance together you
👨‍❤️‍👨	couple with heart: man, man	0	man-heart-man couple_with_heart_man_man couple_with_heart_mm	anniversary babe bae dating kiss love person relationship romance together you
👩‍❤️‍👩	couple with heart: woman, woman	0	woman-heart-woman couple_with_heart_woman_woman couple_with_heart_ww	anniversary babe bae dating kiss love person relationship romance together you
👨‍👩‍👦	family: man, woman, boy	0	man-woman-boy family_man_woman_boy family_mwb	child
👨‍👩‍👧	family: man, woman, girl	0	man-woman-girl family_man_woman_girl family_mwg	child
👨‍👩‍👧‍👦	family: man, woman, girl, boy	0	man-woman-girl-boy family_man_woman_girl_boy family_mwgb	child
👨‍👩‍👦‍👦	family: man, woman, boy, boy	0	man-woman-boy-boy family_man_woman_boy_boy family_mwbb	child
👨‍👩‍👧‍👧	family: man, woman, girl, girl	0	man-woman-girl-girl family_man_woman_girl_girl family_mwgg	child
👨‍👨‍👦	family: man, man, boy	0	man-man-boy family_man_man_boy family_mmb	child
👨‍👨‍👧	family: man, man, girl	0	man-man-girl family_man_man_girl family_mmg	child
👨‍👨‍👧‍👦	family: man, man, girl, boy	0	man-man-girl-boy family_man_man_girl_boy family_mmgb	child
👨‍👨‍👦‍👦	family: man, man, boy, boy	0	man-man-boy-boy family_man_man_boy_boy family_mmbb	child
👨‍👨‍👧‍👧	family: man, man, girl, girl	0	man-man-girl-girl family_man_man_girl_girl family_mmgg	child
👩‍👩‍👦	family: woman, woman, boy	0	woman-woman-boy family_woman_woman_boy family_wwb	child
👩‍👩‍👧	family: woman, woman, girl	0	woman-woman-girl family_woman_woman_girl family_wwg	child
👩‍👩‍👧‍👦	family: woman, woman, girl, boy	0	woman-woman-girl-boy family_woman_woman_girl_boy family_wwgb	child
👩‍👩‍👦‍👦	family: woman, woman, boy, boy	0	woman-woman-boy-boy family_woman_woman_boy_boy family_wwbb	child
👩‍👩‍👧‍👧	family: woman, woman, girl, girl	0	woman-woman-girl-girl family_woman_woman_girl_girl family_wwgg	child
👨‍👦	family: man, boy	0	man-boy family_man_boy family_mb	child
👨‍👦‍👦	family: man, boy, boy	0	man-boy-boy family_man_boy_boy family_mbb	child
👨‍👧	family: man, girl	0	man-girl family_man_girl family_mg	child
👨‍👧‍👦	family: man, girl, boy	0	man-girl-boy family_man_girl_boy family_mgb	child
👨‍👧‍👧	family: man, girl, girl	0	man-girl-girl family_man_girl_girl family_mgg	child
👩‍👦	family: woman, boy	0	woman-boy family_woman_boy family_wb	child
👩‍👦‍👦	family: woman, boy, boy	0	woman-boy-boy family_woman_boy_boy family_wbb	child
👩‍👧	family: woman, girl	0	woman-girl family_woman_girl family_wg	child
👩‍👧‍👦	family: woman, girl, boy	0	woman-girl-boy family_woman_girl_boy family_wgb	child
👩‍👧‍👧	family: woman, girl, girl	0	woman-girl-girl family_woman_girl_girl family_wgg	child
🗣️	speaking head	0	speaking_head_in_silhouette speaking_head	face speak
👤	bust in silhouette	0	bust_in_silhouette	mysterious shadow
👥	busts in silhouette	0	busts_in_silhouette	bff bust everyone friend friends people
🫂	people hugging	0	people_hugging	comfort embrace farewell friendship goodbye hello hug love thanks
👪️	family	0	family	child
🧑‍🧑‍🧒	family: adult, adult, child	0	family_adult_adult_child family_aac	
🧑‍🧑‍🧒‍🧒	family: adult, adult, child, child	0	family_adult_adult_child_child family_aacc	
🧑‍🧒	family: adult, child	0	family_adult_child family_aa family_ac	
🧑‍🧒‍🧒	family: adult, child, child	0	family_adult_child_child family_acc	
👣	footprints	0	footprints	barefoot clothing footprint omw print walk
🫆	fingerprint	0	fingerprint	clue crime detective forensics identity mystery print safety trace
🐵	monkey face	1	monkey_face	animal banana
🐒	monkey	1	monkey	animal banana
🦍	gorilla	1	gorilla	animal
🦧	orangutan	1	orangutan	animal ape monkey
🐶	dog face	1	dog dog_face	adorbs animal pet puppies puppy
🐕️	dog	1	dog2	animal animals dogs pet
🦮	guide dog	1	guide_dog	accessibility animal blind
🐕‍🦺	service dog	1	service_dog	accessibility animal assistance
🐩	poodle	1	poodle	animal dog fluffy
🐺	wolf	1	wolf wolf_face	animal
🦊	fox	1	fox_face fox	animal
🦝	raccoon	1	raccoon	animal curious sly
🐱	cat face	1	cat cat_face	animal kitten kitty pet
🐈️	cat	1	cat2	animal animals cats kitten pet
🐈‍⬛	black cat	1	black_cat	animal feline halloween meow unlucky
🦁	lion	1	lion_face lion	alpha animal leo mane order rawr roar safari strong zodiac
🐯	tiger face	1	tiger tiger_face	animal big cat predator
🐅	tiger	1	tiger2	animal big cat predator zoo
🐆	leopard	1	leopard	animal big cat predator zoo
🐴	horse face	1	horse horse_face	animal dressage equine farm horses
🫎	moose	1	moose	alces animal antlers elk mammal
🫏	donkey	1	donkey	animal ass burro hinny mammal mule stubborn
🐎	horse	1	racehorse	animal equestrian farm racing
🦄	unicorn	1	unicorn_face unicorn	
🦓	zebra	1	zebra_face zebra	animal stripe
🦌	deer	1	deer	animal
🦬	bison	1	bison	animal buffalo herd wisent
🐮	cow face	1	cow cow_face	animal farm milk moo
🐂	ox	1	ox	animal animals bull farm taurus zodiac
🐃	water buffalo	1	water_buffalo	animal zoo
🐄	cow	1	cow2	animal animals farm milk moo
🐷	pig face	1	pig pig_face	animal bacon farm pork
🐖	pig	1	pig2	animal bacon farm pork sow
🐗	boar	1	boar	animal pig
🐽	pig nose	1	pig_nose	animal face farm smell snout
🐏	ram	1	ram	animal aries horns male sheep zodiac zoo
🐑	ewe	1	sheep ewe	animal baa farm female fluffy lamb wool
🐐	goat	1	goat	animal capricorn farm milk zodiac
🐪	camel	1	dromedary_camel	animal desert hump one
🐫	two-hump camel	1	camel	animal bactrian desert two-hump
🦙	llama	1	llama	alpaca animal guanaco vicuña wool
🦒	giraffe	1	giraffe_face giraffe	animal spots
🐘	elephant	1	elephant	animal
🦣	mammoth	1	mammoth	animal extinction large tusk wooly
🦏	rhinoceros	1	rhinoceros rhino	animal
🦛	hippopotamus	1	hippopotamus hippo	animal
🐭	mouse face	1	mouse mouse_face	animal
🐁	mouse	1	mouse2	animal animals
🐀	rat	1	rat	animal
🐹	hamster	1	hamster hamster_face	animal pet
🐰	rabbit face	1	rabbit rabbit_face	animal bunny pet
🐇	rabbit	1	rabbit2	animal bunny pet
🐿️	chipmunk	1	chipmunk	animal squirrel
🦫	beaver	1	beaver	animal dam teeth
🦔	hedgehog	1	hedgehog	animal spiny
🦇	bat	1	bat	animal vampire
🐻	bear	1	bear bear_face	animal grizzly growl honey
🐻‍❄️	polar bear	1	polar_bear polar_bear_face	animal arctic white
🐨	koala	1	koala koala_face	animal australia bear down marsupial under
🐼	panda	1	panda_face panda	animal bamboo
🦥	sloth	1	sloth	lazy slow
🦦	otter	1	otter	animal fishing playful
🦨	skunk	1	skunk	animal stink
🦘	kangaroo	1	kangaroo	animal joey jump marsupial
🦡	badger	1	badger	animal honey pester
🐾	paw prints	1	feet paw_prints	paws print
🦃	turkey	1	turkey	bird gobble thanksgiving
🐔	chicken	1	chicken chicken_face	animal bird ornithology
🐓	rooster	1	rooster	animal bird ornithology
🐣	hatching chick	1	hatching_chick	animal baby bird egg
🐤	baby chick	1	baby_chick	animal bird ornithology
🐥	front-facing baby chick	1	hatched_chick	animal bird front-facing newborn ornithology
🐦️	bird	1	bird bird_face	animal ornithology
🐧	penguin	1	penguin penguin_face	animal antarctica bird ornithology
🕊️	dove	1	dove_of_peace dove	bird fly ornithology
🦅	eagle	1	eagle	animal bird ornithology
🦆	duck	1	duck	animal bird ornithology
🦢	swan	1	swan	animal bird cygnet duckling ornithology ugly
🦉	owl	1	owl	animal bird ornithology wise
🦤	dodo	1	dodo	animal bird extinction large ornithology
🪶	feather	1	feather	bird flight light plumage
🦩	flamingo	1	flamingo	animal bird flamboyant ornithology tropical
🦚	peacock	1	peacock	animal bird colorful ornithology ostentatious peahen pretty proud
🦜	parrot	1	parrot	animal bird ornithology pirate talk
🪽	wing	1	wing	angelic ascend aviation bird fly flying heavenly mythology soar
🐦‍⬛	black bird	1	black_bird	animal beak caw corvid crow ornithology raven rook
🪿	goose	1	goose	animal bird duck flock fowl gaggle gander geese honk ornithology silly
🐦‍🔥	phoenix	1	phoenix	ascend ascension emerge fantasy firebird glory immortal rebirth reincarnation reinvent renewal revival revive rise transform
🐸	frog	1	frog frog_face	animal
🐊	crocodile	1	crocodile	animal zoo
🐢	turtle	1	turtle	animal terrapin tortoise
🦎	lizard	1	lizard	animal reptile
🐍	snake	1	snake	animal bearer ophiuchus serpent zodiac
🐲	dragon face	1	dragon_face	animal fairy fairytale tale
🐉	dragon	1	dragon	animal fairy fairytale knights tale
🦕	sauropod	1	sauropod	brachiosaurus brontosaurus dinosaur diplodocus
🦖	t-rex	1	t-rex trex	dinosaur t-rex tyrannosaurus
🐳	spouting whale	1	whale spouting_whale	animal beach face ocean
🐋	whale	1	whale2	animal beach ocean
🐬	dolphin	1	dolphin flipper	animal beach ocean
🫍	orca	1	orca	marine ocean whale
🦭	seal	1	seal	animal lion ocean sea
🐟️	fish	1	fish	animal dinner fishes fishing pisces zodiac
🐠	tropical fish	1	tropical_fish	animal fishes
🐡	blowfish	1	blowfish	animal fish
🦈	shark	1	shark	animal fish
🐙	octopus	1	octopus	animal creature ocean
🐚	spiral shell	1	shell	animal beach conch sea
🪸	coral	1	coral	change climate ocean reef sea
🪼	jellyfish	1	jellyfish	animal aquarium burn invertebrate jelly life marine ocean ouch plankton sea sting stinger tentacles
🦀	crab	1	crab	cancer zodiac
🦞	lobster	1	lobster	animal bisque claws seafood
🦐	shrimp	1	shrimp	food shellfish small
🦑	squid	1	squid	animal food mollusk
🦪	oyster	1	oyster	diving pearl
🐌	snail	1	snail	animal escargot garden nature slug
🦋	butterfly	1	butterfly	insect pretty
🐛	bug	1	bug	animal garden insect
🐜	ant	1	ant	animal garden insect
🐝	honeybee	1	bee honeybee	animal bumblebee honey insect nature spring
🪲	beetle	1	beetle	animal bug insect
🐞	lady beetle	1	ladybug lady_beetle	animal garden insect ladybird nature
🦗	cricket	1	cricket	animal bug grasshopper insect orthoptera
🪳	cockroach	1	cockroach	animal insect pest roach
🕷️	spider	1	spider	animal insect
🕸️	spider web	1	spider_web	
🦂	scorpion	1	scorpion	scorpio scorpius zodiac
🦟	mosquito	1	mosquito	bite disease fever insect malaria pest virus
🪰	fly	1	fly	animal disease insect maggot pest rotting
🪱	worm	1	worm	animal annelid earthworm parasite
🦠	microbe	1	microbe	amoeba bacteria science virus
💐	bouquet	1	bouquet	anniversary birthday date flower love plant romance
🌸	cherry blossom	1	cherry_blossom	flower plant spring springtime
💮	white flower	1	white_flower	
🪷	lotus	1	lotus	beauty buddhism calm flower hinduism peace purity serenity
🏵️	rosette	1	rosette	plant
🌹	rose	1	rose	beauty elegant flower love plant red valentine
🥀	wilted flower	1	wilted_flower	dying
🌺	hibiscus	1	hibiscus	flower plant
🌻	sunflower	1	sunflower	flower outdoors plant sun
🌼	blossom	1	blossom	buttercup dandelion flower plant
🌷	tulip	1	tulip	blossom flower growth plant
🪻	hyacinth	1	hyacinth	bloom bluebonnet flower indigo lavender lilac lupine plant purple shrub snapdragon spring violet
🌱	seedling	1	seedling	plant sapling sprout young
🪴	potted plant	1	potted_plant	decor grow house nurturing pot
🌲	evergreen tree	1	evergreen_tree	christmas forest pine
🌳	deciduous tree	1	deciduous_tree	forest green habitat shedding
🌴	palm tree	1	palm_tree	beach plant tropical
🌵	cactus	1	cactus	desert drought nature plant
🌾	sheaf of rice	1	ear_of_rice sheaf_of_rice	grain grains plant
🌿	herb	1	herb	leaf plant
☘️	shamrock	1	shamrock	irish plant
🍀	four leaf clover	1	four_leaf_clover	4 four-leaf irish lucky plant
🍁	maple leaf	1	maple_leaf	falling
🍂	fallen leaf	1	fallen_leaf	autumn fall falling
🍃	leaf fluttering in wind	1	leaves	blow flutter
🪹	empty nest	1	empty_nest nest	branch home nesting
🪺	nest with eggs	1	nest_with_eggs	bird branch egg nesting
🍄	mushroom	1	mushroom	fungus toadstool
🪾	leafless tree	1	leafless_tree	bare barren branches dead drought trunk winter wood
🍇	grapes	2	grapes	dionysus fruit grape
🍈	melon	2	melon	cantaloupe fruit
🍉	watermelon	2	watermelon	fruit
🍊	tangerine	2	tangerine mandarin orange	c citrus fruit nectarine vitamin
🍋	lemon	2	lemon	citrus fruit sour
🍋‍🟩	lime	2	lime	acidity citrus cocktail fruit garnish key margarita mojito refreshing salsa sour tangy tequila tropical zest
🍌	banana	2	banana	fruit potassium
🍍	pineapple	2	pineapple	colada fruit pina tropical
🥭	mango	2	mango	food fruit tropical
🍎	red apple	2	apple red_apple	diet food fruit health ripe
🍏	green apple	2	green_apple	fruit
🍐	pear	2	pear	fruit
🍑	peach	2	peach	fruit
🍒	cherries	2	cherries	berries cherry fruit red
🍓	strawberry	2	strawberry	berry fruit
🫐	blueberries	2	blueberries	berries berry bilberry blue blueberry food fruit
🥝	kiwi fruit	2	kiwifruit kiwi_fruit kiwi	food
🍅	tomato	2	tomato	food fruit vegetable
🫒	olive	2	olive	food
🥥	coconut	2	coconut	colada palm piña
🥑	avocado	2	avocado	food fruit
🍆	eggplant	2	eggplant	aubergine vegetable
🥔	potato	2	potato	food vegetable
🥕	carrot	2	carrot	food vegetable
🌽	ear of corn	2	corn ear_of_corn	crops farm maize maze
🌶️	hot pepper	2	hot_pepper	
🫑	bell pepper	2	bell_pepper	capsicum food vegetable
🥒	cucumber	2	cucumber	food pickle vegetable
🥬	leafy green	2	leafy_green	bok burgers cabbage choy kale lettuce salad
🥦	broccoli	2	broccoli	cabbage wild
🧄	garlic	2	garlic	flavoring
🧅	onion	2	onion	flavoring
🥜	peanuts	2	peanuts	food nut peanut vegetable
🫘	beans	2	beans	food kidney legume small
🌰	chestnut	2	chestnut	almond plant
🫚	ginger root	2	ginger_root ginger	beer health herb natural spice
🫛	pea pod	2	pea_pod pea	beans beanstalk edamame legume soybean vegetable veggie
🍄‍🟫	brown mushroom	2	brown_mushroom	food fungi fungus nature pizza portobello shiitake shroom spore sprout toppings truffle vegetable vegetarian veggie
🫜	root vegetable	2	root_vegetable	beet food garden radish salad turnip vegetarian
🍞	bread	2	bread	carbs food grain loaf restaurant toast wheat
🥐	croissant	2	croissant	bread breakfast crescent food french roll
🥖	baguette bread	2	baguette_bread	food french
🫓	flatbread	2	flatbread	arepa bread food gordita lavash naan pita
🥨	pretzel	2	pretzel	convoluted twisted
🥯	bagel	2	bagel	bakery bread breakfast schmear
🥞	pancakes	2	pancakes	breakfast crêpe food hotcake pancake
🧇	waffle	2	waffle	breakfast indecisive iron
🧀	cheese wedge	2	cheese_wedge cheese	
🍖	meat on bone	2	meat_on_bone	
🍗	poultry leg	2	poultry_leg	bone chicken drumstick hungry turkey
🥩	cut of meat	2	cut_of_meat	chop lambchop porkchop red steak
🥓	bacon	2	bacon	breakfast food meat
🍔	hamburger	2	hamburger	burger eat fast food hungry
🍟	french fries	2	fries french_fries	fast food
🍕	pizza	2	pizza	cheese food hungry pepperoni slice
🌭	hot dog	2	hotdog	frankfurter sausage
🥪	sandwich	2	sandwich	bread
🌮	taco	2	taco	mexican
🌯	burrito	2	burrito	mexican wrap
🫔	tamale	2	tamale	food mexican pamonha wrapped
🥙	stuffed flatbread	2	stuffed_flatbread	falafel food gyro kebab
🧆	falafel	2	falafel	chickpea meatball
🥚	egg	2	egg	breakfast food
🍳	cooking	2	fried_egg cooking	breakfast easy fry frying over pan restaurant side sunny up
🥘	shallow pan of food	2	shallow_pan_of_food	casserole paella
🍲	pot of food	2	stew pot_of_food	soup
🫕	fondue	2	fondue	cheese chocolate food melted pot ski
🥣	bowl with spoon	2	bowl_with_spoon	breakfast cereal congee oatmeal porridge
🥗	green salad	2	green_salad salad	food
🍿	popcorn	2	popcorn	corn movie pop
🧈	butter	2	butter	dairy
🧂	salt	2	salt	condiment flavor mad salty shaker taste upset
🥫	canned food	2	canned_food	can
🍱	bento box	2	bento bento_box	food
🍘	rice cracker	2	rice_cracker	food
🍙	rice ball	2	rice_ball	food japanese
🍚	cooked rice	2	rice cooked_rice	food
🍛	curry rice	2	curry curry_rice	food
🍜	steaming bowl	2	ramen steaming_bowl	chopsticks food noodle pho soup
🍝	spaghetti	2	spaghetti	food meatballs pasta restaurant
🍠	roasted sweet potato	2	sweet_potato	food
🍢	oden	2	oden	food kebab restaurant seafood skewer stick
🍣	sushi	2	sushi	food
🍤	fried shrimp	2	fried_shrimp	prawn tempura
🍥	fish cake with swirl	2	fish_cake	food pastry restaurant
🥮	moon cake	2	moon_cake	autumn festival yuèbǐng
🍡	dango	2	dango	dessert japanese skewer stick sweet
🥟	dumpling	2	dumpling	empanada gyōza jiaozi pierogi potsticker
🥠	fortune cookie	2	fortune_cookie	prophecy
🥡	takeout box	2	takeout_box	chopsticks delivery food oyster pail
🍦	soft ice cream	2	icecream soft_serve	dessert food restaurant sweet
🍧	shaved ice	2	shaved_ice	dessert restaurant sweet
🍨	ice cream	2	ice_cream	dessert food restaurant sweet
🍩	doughnut	2	doughnut	breakfast dessert donut food sweet
🍪	cookie	2	cookie	chip chocolate dessert sweet
🎂	birthday cake	2	birthday birthday_cake	bday celebration dessert happy pastry sweet
🍰	shortcake	2	cake shortcake	dessert pastry slice sweet
🧁	cupcake	2	cupcake	bakery dessert sprinkles sugar sweet treat
🥧	pie	2	pie	apple filling fruit meat pastry pumpkin slice
🍫	chocolate bar	2	chocolate_bar	candy dessert halloween sweet tooth
🍬	candy	2	candy	cavities dessert halloween restaurant sweet tooth wrapper
🍭	lollipop	2	lollipop	candy dessert food restaurant sweet
🍮	custard	2	custard	dessert pudding sweet
🍯	honey pot	2	honey_pot	barrel bear food honeypot jar sweet
🍼	baby bottle	2	baby_bottle	babies birth born drink infant milk newborn
🥛	glass of milk	2	glass_of_milk milk_glass milk	drink
☕️	hot beverage	2	coffee	cafe caffeine chai drink morning steaming tea
🫖	teapot	2	teapot	brew drink food pot tea
🍵	teacup without handle	2	tea	beverage cup drink oolong
🍶	sake	2	sake	bar beverage bottle cup drink restaurant
🍾	bottle with popping cork	2	champagne	bar drink
🍷	wine glass	2	wine_glass	alcohol bar beverage booze club drink drinking drinks restaurant
🍸️	cocktail glass	2	cocktail	alcohol bar booze club drink drinking drinks mad martini men
🍹	tropical drink	2	tropical_drink	alcohol bar booze club cocktail drinking drinks drunk mai party tai tropics
🍺	beer mug	2	beer	alcohol ale bar booze drink drinking drinks octoberfest oktoberfest pint stein summer
🍻	clinking beer mugs	2	beers	alcohol bar booze bottoms cheers clink drinking drinks
🥂	clinking glasses	2	clinking_glasses	celebrate clink drink glass
🥃	tumbler glass	2	tumbler_glass whisky	liquor scotch shot whiskey
🫗	pouring liquid	2	pouring_liquid pour	accident drink empty glass oops spill water
🥤	cup with straw	2	cup_with_straw	drink juice malt soda soft water
🧋	bubble tea	2	bubble_tea boba_drink	food milk pearl
🧃	beverage box	2	beverage_box juice_box	straw sweet
🧉	mate	2	mate_drink mate	
🧊	ice	2	ice_cube ice	cold iceberg
🥢	chopsticks	2	chopsticks	hashi jeotgarak kuaizi
🍽️	fork and knife with plate	2	knife_fork_plate plate_with_cutlery fork_knife_plate	cooking dinner eat
🍴	fork and knife	2	fork_and_knife	breakfast breaky cooking cutlery delicious dinner eat feed food hungry lunch restaurant yum yummy
🥄	spoon	2	spoon	eat tableware
🔪	kitchen knife	2	hocho knife	chef cooking tool weapon
🫙	jar	2	jar	condiment container empty nothing sauce store
🏺	amphora	2	amphora	aquarius cooking drink jug tool weapon zodiac
🌍️	globe showing europe-africa	4	earth_africa earth_europe	europe-africa world
🌎️	globe showing americas	4	earth_americas	world
🌏️	globe showing asia-australia	4	earth_asia	asia-australia world
🌐	globe with meridians	4	globe_with_meridians	earth internet web world worldwide
🗺️	world map	4	world_map	
🗾	map of japan	4	japan japan_map	
🧭	compass	4	compass	direction magnetic navigation orienteering
🏔️	snow-capped mountain	4	snow_capped_mountain mountain_snow	cold snow-capped
⛰️	mountain	4	mountain	
🛘	landslide	4	landslide	avalanche danger disaster earthquake mountain mudslide rocks
🌋	volcano	4	volcano	eruption mountain nature
🗻	mount fuji	4	mount_fuji	mountain nature
🏕️	camping	4	camping	
🏖️	beach with umbrella	4	beach_with_umbrella beach_umbrella beach	
🏜️	desert	4	desert	
🏝️	desert island	4	desert_island island	
🏞️	national park	4	national_park	
🏟️	stadium	4	stadium	
🏛️	classical building	4	classical_building	
🏗️	building construction	4	building_construction construction_site	crane
🧱	brick	4	bricks	clay mortar wall
🪨	rock	4	rock	boulder heavy solid stone tough
🪵	wood	4	wood	log lumber timber
🛖	hut	4	hut	home house roundhouse shelter yurt
🏘️	houses	4	house_buildings houses homes	
🏚️	derelict house	4	derelict_house_building derelict_house house_abandoned	home
🏠️	house	4	house	building country heart home ranch settle simple suburban suburbia where
🏡	house with garden	4	house_with_garden	building country heart home ranch settle simple suburban suburbia where
🏢	office building	4	office	city cubical job
🏣	japanese post office	4	post_office	building
🏤	post office	4	european_post_office	building
🏥	hospital	4	hospital	building doctor medicine
🏦	bank	4	bank	building
🏨	hotel	4	hotel	building
🏩	love hotel	4	love_hotel	building
🏪	convenience store	4	convenience_store	24 building hours
🏫	school	4	school	building
🏬	department store	4	department_store	building
🏭️	factory	4	factory	building
🏯	japanese castle	4	japanese_castle	building
🏰	castle	4	european_castle castle	building
💒	wedding	4	wedding	chapel hitched nuptials romance
🗼	tokyo tower	4	tokyo_tower	
🗽	statue of liberty	4	statue_of_liberty	new ny nyc york
⛪️	church	4	church	bless chapel christian cross religion
🕌	mosque	4	mosque	islam masjid muslim religion
🛕	hindu temple	4	hindu_temple	
🕍	synagogue	4	synagogue	jew jewish judaism religion temple
⛩️	shinto shrine	4	shinto_shrine	religion
🕋	kaaba	4	kaaba	hajj islam muslim religion umrah
⛲️	fountain	4	fountain	
⛺️	tent	4	tent	camping
🌁	foggy	4	foggy	fog
🌃	night with stars	4	night_with_stars	star
🏙️	cityscape	4	cityscape	city
🌄	sunrise over mountains	4	sunrise_over_mountains	morning sun
🌅	sunrise	4	sunrise	morning nature sun
🌆	cityscape at dusk	4	city_sunset city_dusk	building evening landscape sun
🌇	sunset	4	city_sunrise	building dusk sun
🌉	bridge at night	4	bridge_at_night	
♨️	hot springs	4	hotsprings	steaming
🎠	carousel horse	4	carousel_horse	entertainment
🛝	playground slide	4	playground_slide slide	amusement park play playing sliding theme
🎡	ferris wheel	4	ferris_wheel	amusement park theme
🎢	roller coaster	4	roller_coaster	amusement park theme
💈	barber pole	4	barber barber_pole	cut fresh haircut shave
🎪	circus tent	4	circus_tent	
🚂	locomotive	4	steam_locomotive	caboose engine railway train trains travel
🚃	railway car	4	railway_car	electric train tram travel trolleybus
🚄	high-speed train	4	bullettrain_side	high-speed railway shinkansen
🚅	bullet train	4	bullettrain_front	high-speed nose railway shinkansen speed travel
🚆	train	4	train2	arrived choo railway
🚇️	metro	4	metro	subway travel
🚈	light rail	4	light_rail	arrived monorail railway
🚉	station	4	station	railway train
🚊	tram	4	tram	trolleybus
🚝	monorail	4	monorail	vehicle
🚞	mountain railway	4	mountain_railway	car trip
🚋	tram car	4	train tram_car	bus trolley trolleybus
🚌	bus	4	bus	school vehicle
🚍️	oncoming bus	4	oncoming_bus	cars
🚎	trolleybus	4	trolleybus	bus tram trolley
🚐	minibus	4	minibus	bus drive van vehicle
🚑️	ambulance	4	ambulance	emergency vehicle
🚒	fire engine	4	fire_engine	truck
🚓	police car	4	police_car	5–0 cops patrol
🚔️	oncoming police car	4	oncoming_police_car	
🚕	taxi	4	taxi	cab cabbie car drive vehicle yellow
🚖	oncoming taxi	4	oncoming_taxi	cab cabbie cars drove hail yellow
🚗	automobile	4	car red_car	driving vehicle
🚘️	oncoming automobile	4	oncoming_automobile	car cars drove vehicle
🚙	sport utility vehicle	4	blue_car suv	drive recreational sportutility
🛻	pickup truck	4	pickup_truck	automobile car flatbed pick-up transportation
🚚	delivery truck	4	truck delivery_truck	car drive vehicle
🚛	articulated lorry	4	articulated_lorry	car drive move semi truck vehicle
🚜	tractor	4	tractor	vehicle
🏎️	racing car	4	racing_car	zoom
🏍️	motorcycle	4	racing_motorcycle motorcycle	
🛵	motor scooter	4	motor_scooter	
🦽	manual wheelchair	4	manual_wheelchair	accessibility
🦼	motorized wheelchair	4	motorized_wheelchair	accessibility
🛺	auto rickshaw	4	auto_rickshaw	tuk
🚲️	bicycle	4	bike bicycle	class cycle cycling cyclist gang ride spin spinning
🛴	kick scooter	4	scooter kick_scooter	
🛹	skateboard	4	skateboard	board skate skater wheels
🛼	roller skate	4	roller_skate	blades skates sport
🚏	bus stop	4	busstop	
🛣️	motorway	4	motorway	highway road
🛤️	railway track	4	railway_track	train
🛢️	oil drum	4	oil_drum	
⛽️	fuel pump	4	fuelpump	diesel gas gasoline station
🛞	wheel	4	wheel	car circle tire turn vehicle
🚨	police car light	4	rotating_light	alarm alert beacon emergency revolving siren
🚥	horizontal traffic light	4	traffic_light	intersection signal stop stoplight
🚦	vertical traffic light	4	vertical_traffic_light	drove intersection signal stop stoplight
🛑	stop sign	4	octagonal_sign stop_sign	
🚧	construction	4	construction	barrier
⚓️	anchor	4	anchor	ship tool
🛟	ring buoy	4	ring_buoy lifebuoy	float life lifesaver preserver rescue safety save saver swim
⛵️	sailboat	4	boat sailboat	resort sailing sea yacht
🛶	canoe	4	canoe	boat
🚤	speedboat	4	speedboat	billionaire boat lake luxury millionaire summer travel
🛳️	passenger ship	4	passenger_ship cruise_ship	
⛴️	ferry	4	ferry	boat passenger
🛥️	motor boat	4	motor_boat motorboat	
🚢	ship	4	ship	boat passenger travel
✈️	airplane	4	airplane	aeroplane fly flying jet plane travel
🛩️	small airplane	4	small_airplane	aeroplane plane
🛫	airplane departure	4	airplane_departure flight_departure	aeroplane check-in departures plane
🛬	airplane arrival	4	airplane_arriving flight_arrival	aeroplane arrivals landing plane
🪂	parachute	4	parachute	hang-glide parasail skydive
💺	seat	4	seat	chair
🚁	helicopter	4	helicopter	copter roflcopter travel vehicle
🚟	suspension railway	4	suspension_railway	
🚠	mountain cableway	4	mountain_cableway	cable gondola lift ski
🚡	aerial tramway	4	aerial_tramway	cable car gondola ropeway
🛰️	satellite	4	satellite artificial_satellite	space
🚀	rocket	4	rocket	launch rockets space travel
🛸	flying saucer	4	flying_saucer	aliens extra terrestrial ufo
🛎️	bellhop bell	4	bellhop_bell bellhop	hotel
🧳	luggage	4	luggage	bag packing roller suitcase travel
⌛️	hourglass done	4	hourglass	sand time timer
⏳️	hourglass not done	4	hourglass_flowing_sand	hours timer waiting yolo
⌚️	watch	4	watch	clock time
⏰️	alarm clock	4	alarm_clock	hours hrs late time waiting
⏱️	stopwatch	4	stopwatch	clock time
⏲️	timer clock	4	timer_clock	
🕰️	mantelpiece clock	4	mantelpiece_clock clock	time
🕛️	twelve o’clock	4	clock12	12 12:00 o’clock time
🕧️	twelve-thirty	4	clock1230	12 12:30 30 clock time
🕐️	one o’clock	4	clock1	1 1:00 o’clock time
🕜️	one-thirty	4	clock130	1 1:30 30 clock time
🕑️	two o’clock	4	clock2	2 2:00 o’clock time
🕝️	two-thirty	4	clock230	2 2:30 30 clock time
🕒️	three o’clock	4	clock3	3 3:00 o’clock time
🕞️	three-thirty	4	clock330	3 30 3:30 clock time
🕓️	four o’clock	4	clock4	4 4:00 o’clock time
🕟️	four-thirty	4	clock430	30 4 4:30 clock time
🕔️	five o’clock	4	clock5	5 5:00 o’clock time
🕠️	five-thirty	4	clock530	30 5 5:30 clock time
🕕️	six o’clock	4	clock6	6 6:00 o’clock time
🕡️	six-thirty	4	clock630	30 6 6:30 clock
🕖️	seven o’clock	4	clock7	0 7 7:00 o’clock
🕢️	seven-thirty	4	clock730	30 7 7:30 clock
🕗️	eight o’clock	4	clock8	8 8:00 o’clock time
🕣️	eight-thirty	4	clock830	30 8 8:30 clock time
🕘️	nine o’clock	4	clock9	9 9:00 o’clock time
🕤️	nine-thirty	4	clock930	30 9 9:30 clock time
🕙️	ten o’clock	4	clock10	0 10 10:00 o’clock
🕥️	ten-thirty	4	clock1030	10 10:30 30 clock time
🕚️	eleven o’clock	4	clock11	11 11:00 o’clock time
🕦️	eleven-thirty	4	clock1130	11 11:30 30 clock time
🌑	new moon	4	new_moon	dark space
🌒	waxing crescent moon	4	waxing_crescent_moon	dreams space
🌓	first quarter moon	4	first_quarter_moon	space
🌔	waxing gibbous moon	4	moon waxing_gibbous_moon	space
🌕️	full moon	4	full_moon	space
🌖	waning gibbous moon	4	waning_gibbous_moon	space
🌗	last quarter moon	4	last_quarter_moon	space
🌘	waning crescent moon	4	waning_crescent_moon	space
🌙	crescent moon	4	crescent_moon	ramadan space
🌚	new moon face	4	new_moon_with_face	space
🌛	first quarter moon face	4	first_quarter_moon_with_face	space
🌜️	last quarter moon face	4	last_quarter_moon_with_face	dreams
🌡️	thermometer	4	thermometer	weather
☀️	sun	4	sunny sun	bright rays space weather
🌝	full moon face	4	full_moon_with_face	bright
🌞	sun with face	4	sun_with_face	beach bright day heat shine sunny sunshine weather
🪐	ringed planet	4	ringed_planet saturn	saturnine
⭐️	star	4	star	astronomy medium stars white
🌟	glowing star	4	star2 glowing_star	glittery glow night shining sparkle win
🌠	shooting star	4	stars shooting_star	falling night space
🌌	milky way	4	milky_way	space
☁️	cloud	4	cloud	weather
⛅️	sun behind cloud	4	partly_sunny	cloudy weather
⛈️	cloud with lightning and rain	4	thunder_cloud_and_rain cloud_with_lightning_and_rain stormy	thunderstorm
🌤️	sun behind small cloud	4	mostly_sunny sun_small_cloud sun_behind_small_cloud	weather
🌥️	sun behind large cloud	4	barely_sunny sun_behind_cloud sun_behind_large_cloud cloudy	weather
🌦️	sun behind rain cloud	4	partly_sunny_rain sun_behind_rain_cloud sun_and_rain	weather
🌧️	cloud with rain	4	rain_cloud cloud_with_rain rainy	weather
🌨️	cloud with snow	4	snow_cloud cloud_with_snow snowy	cold weather
🌩️	cloud with lightning	4	lightning lightning_cloud cloud_with_lightning	weather
🌪️	tornado	4	tornado tornado_cloud	weather whirlwind
🌫️	fog	4	fog	cloud weather
🌬️	wind face	4	wind_blowing_face wind_face	blow cloud
🌀	cyclone	4	cyclone	dizzy hurricane twister typhoon weather
🌈	rainbow	4	rainbow	gay genderqueer glbt glbtq lesbian lgbt lgbtq lgbtqia nature pride queer rain trans transgender weather
🌂	closed umbrella	4	closed_umbrella	clothing rain
☂️	umbrella	4	umbrella open_umbrella	clothing rain
☔️	umbrella with rain drops	4	umbrella_with_rain_drops umbrella_with_rain	clothing drop weather
⛱️	umbrella on ground	4	umbrella_on_ground parasol_on_ground	rain sun
⚡️	high voltage	4	zap high_voltage	danger electric electricity lightning nature thunder thunderbolt
❄️	snowflake	4	snowflake	cold snow weather
☃️	snowman	4	snowman snowman_with_snow snowman2	cold man
⛄️	snowman without snow	4	snowman_without_snow	cold man
☄️	comet	4	comet	space
🔥	fire	4	fire	af burn flame hot lit litaf tool
💧	droplet	4	droplet	cold comic drop nature sad sweat tear water weather
🌊	water wave	4	ocean water_wave	nature surf surfer surfing
🎃	jack-o-lantern	3	jack_o_lantern	celebration halloween pumpkin
🎄	christmas tree	3	christmas_tree	celebration
🎆	fireworks	3	fireworks	boom celebration entertainment yolo
🎇	sparkler	3	sparkler	boom celebration fireworks sparkle
🧨	firecracker	3	firecracker	dynamite explosive fire fireworks light pop popping spark
✨️	sparkles	3	sparkles	* magic sparkle star
🎈	balloon	3	balloon	birthday celebrate celebration
🎉	party popper	3	tada party party_popper	awesome birthday celebrate celebration excited hooray woohoo
🎊	confetti ball	3	confetti_ball	celebrate celebration party woohoo
🎋	tanabata tree	3	tanabata_tree	banner celebration japanese
🎍	pine decoration	3	bamboo	celebration japanese plant
🎎	japanese dolls	3	dolls	celebration doll festival
🎏	carp streamer	3	flags carp_streamer	celebration
🎐	wind chime	3	wind_chime	bell celebration
🎑	moon viewing ceremony	3	rice_scene moon_ceremony	celebration
🧧	red envelope	3	red_envelope	gift good hóngbāo lai luck money see
🎀	ribbon	3	ribbon	celebration
🎁	wrapped gift	3	gift	birthday bow box celebration christmas present surprise
🎗️	reminder ribbon	3	reminder_ribbon	celebration
🎟️	admission tickets	3	admission_tickets tickets	ticket
🎫	ticket	3	ticket	admission stub
🎖️	military medal	3	medal medal_military military_medal	award celebration
🏆️	trophy	3	trophy	champion champs prize slay sport victory win winning
🏅	sports medal	3	sports_medal medal_sports	award gold winner
🥇	1st place medal	3	first_place_medal 1st_place_medal 1st	gold
🥈	2nd place medal	3	second_place_medal 2nd_place_medal 2nd	silver
🥉	3rd place medal	3	third_place_medal 3rd_place_medal 3rd	bronze
⚽️	soccer ball	3	soccer	football futbol sport
⚾️	baseball	3	baseball	ball sport
🥎	softball	3	softball	ball glove sports underarm
🏀	basketball	3	basketball	ball hoop sport
🏐	volleyball	3	volleyball	ball game
🏈	american football	3	football	ball bowl sport super
🏉	rugby football	3	rugby_football	ball sport
🎾	tennis	3	tennis	ball racquet sport
🥏	flying disc	3	flying_disc	ultimate
🎳	bowling	3	bowling	ball game sport strike
🏏	cricket game	3	cricket_bat_and_ball cricket_game	
🏑	field hockey	3	field_hockey_stick_and_ball field_hockey	game
🏒	ice hockey	3	ice_hockey_stick_and_puck ice_hockey hockey	game
🥍	lacrosse	3	lacrosse	ball goal sports stick
🏓	ping pong	3	table_tennis_paddle_and_ball ping_pong	bat game pingpong
🏸	badminton	3	badminton_racquet_and_shuttlecock badminton	birdie game
🥊	boxing glove	3	boxing_glove	
🥋	martial arts uniform	3	martial_arts_uniform	judo karate taekwondo
🥅	goal net	3	goal_net	
⛳️	flag in hole	3	golf	sport
⛸️	ice skate	3	ice_skate	skating
🎣	fishing pole	3	fishing_pole_and_fish fishing_pole	entertainment sport
🤿	diving mask	3	diving_mask	scuba snorkeling
🎽	running shirt	3	running_shirt_with_sash running_shirt	athletics
🎿	skis	3	ski	snow sport
🛷	sled	3	sled	luge sledge sleigh snow toboggan
🥌	curling stone	3	curling_stone	game rock
🎯	bullseye	3	dart bullseye direct_hit	bull entertainment game target
🪀	yo-yo	3	yo-yo yo_yo	fluctuate toy
🪁	kite	3	kite	fly soar
🔫	water pistol	3	gun pistol	handgun revolver tool weapon
🎱	pool 8 ball	3	8ball billiards	billiard eight game
🔮	crystal ball	3	crystal_ball	fairy fairytale fantasy fortune future magic tale tool
🪄	magic wand	3	magic_wand	magician witch wizard
🎮️	video game	3	video_game controller	entertainment
🕹️	joystick	3	joystick	game video videogame
🎰	slot machine	3	slot_machine	casino gamble gambling game slots
🎲	game die	3	game_die	dice entertainment
🧩	puzzle piece	3	jigsaw puzzle_piece	clue interlocking
🧸	teddy bear	3	teddy_bear	plaything plush stuffed toy
🪅	piñata	3	pinata	candy celebrate celebration cinco de festive mayo party pinada
🪩	mirror ball	3	mirror_ball disco disco_ball	dance glitter party
🪆	nesting dolls	3	nesting_dolls	babooshka baboushka babushka doll matryoshka russia
♠️	spade suit	3	spades	card game
♥️	heart suit	3	hearts	card emotion game
♦️	diamond suit	3	diamonds	card game
♣️	club suit	3	clubs	card game
♟️	chess pawn	3	chess_pawn	dupe expendable
🃏	joker	3	black_joker	card game wildcard
🀄️	mahjong red dragon	3	mahjong	game
🎴	flower playing cards	3	flower_playing_cards	card game japanese
🎭️	performing arts	3	performing_arts	actor actress art entertainment mask theater theatre thespian
🖼️	framed picture	3	frame_with_picture framed_picture	art museum painting
🎨	artist palette	3	art palette	artsy arty colorful creative entertainment museum painter painting
🧵	thread	3	thread	needle sewing spool string
🪡	sewing needle	3	sewing_needle	embroidery sew stitches sutures tailoring thread
🧶	yarn	3	yarn	ball crochet knit
🪢	knot	3	knot	cord rope tangled tie twine twist
👓️	glasses	5	eyeglasses glasses	clothing eye eyewear
🕶️	sunglasses	5	dark_sunglasses	eye eyewear glasses
🥽	goggles	5	goggles	dive eye protection scuba swimming welding
🥼	lab coat	5	lab_coat	clothes doctor dr experiment jacket scientist white
🦺	safety vest	5	safety_vest	emergency
👔	necktie	5	necktie	clothing employed serious shirt tie
👕	t-shirt	5	shirt tshirt	blue casual clothes clothing collar dressed shopping weekend
👖	jeans	5	jeans	blue casual clothes clothing denim dressed pants shopping trousers weekend
🧣	scarf	5	scarf	bundle cold neck up
🧤	gloves	5	gloves	hand
🧥	coat	5	coat	brr bundle cold jacket up
🧦	socks	5	socks	stocking
👗	dress	5	dress	clothes clothing dressed fancy shopping
👘	kimono	5	kimono	clothing comfortable
🥻	sari	5	sari	clothing dress
🩱	one-piece swimsuit	5	one-piece_swimsuit one_piece_swimsuit	bathing one-piece suit
🩲	briefs	5	briefs swim_brief	bathing one-piece suit swimsuit underwear
🩳	shorts	5	shorts	bathing pants suit swimsuit underwear
👙	bikini	5	bikini	bathing beach clothing pool suit swim
👚	woman’s clothes	5	womans_clothes	blouse clothing collar dress dressed lady shirt shopping woman’s
🪭	folding hand fan	5	folding_hand_fan folding_fan	clack clap cool cooling dance flirt flutter hot shy
👛	purse	5	purse	clothes clothing coin dress fancy handbag shopping
👜	handbag	5	handbag	bag clothes clothing dress lady purse shopping
👝	clutch bag	5	pouch clutch_bag	clothes clothing dress handbag purse
🛍️	shopping bags	5	shopping_bags shopping	bag hotel
🎒	backpack	5	school_satchel backpack	backpacking bag bookbag education rucksack
🩴	thong sandal	5	thong_sandal	beach flip flop sandals shoe thongs zōri
👞	man’s shoe	5	mans_shoe shoe	brown clothes clothing feet foot kick man’s shoes shopping
👟	running shoe	5	athletic_shoe sneaker	clothes clothing fast kick shoes shopping tennis
🥾	hiking boot	5	hiking_boot	backpacking brown camping outdoors shoe
🥿	flat shoe	5	womans_flat_shoe flat_shoe	ballet comfy flats slip-on slipper
👠	high-heeled shoe	5	high_heel	clothes clothing dress fashion heels high-heeled shoes shopping stiletto woman
👡	woman’s sandal	5	sandal	clothing shoe woman’s
🩰	ballet shoes	5	ballet_shoes	dance
👢	woman’s boot	5	boot	clothes clothing dress shoe shoes shopping woman’s
🪮	hair pick	5	hair_pick	afro comb groom
👑	crown	5	crown	clothing family king medieval queen royal royalty win
👒	woman’s hat	5	womans_hat	clothes clothing garden hats party woman’s
🎩	top hat	5	tophat top_hat	clothes clothing fancy formal magic
🎓️	graduation cap	5	mortar_board graduation_cap	celebration clothing education hat scholar
🧢	billed cap	5	billed_cap	baseball bent dad hat
🪖	military helmet	5	military_helmet	army soldier war warrior
⛑️	rescue worker’s helmet	5	helmet_with_white_cross rescue_worker_helmet helmet_with_cross	aid face hat worker’s
📿	prayer beads	5	prayer_beads	clothing necklace religion
💄	lipstick	5	lipstick	cosmetics date makeup
💍	ring	5	ring	diamond engaged engagement married romance shiny sparkling wedding
💎	gem stone	5	gem	diamond engagement jewel money romance wedding
🔇	muted speaker	5	mute no_sound	quiet silent
🔈️	speaker low volume	5	speaker low_volume quiet_sound	soft
🔉	speaker medium volume	5	sound medium_volumne	
🔊	speaker high volume	5	loud_sound high_volume	music
📢	loudspeaker	5	loudspeaker	address communication loud public sound
📣	megaphone	5	mega megaphone	cheering sound
📯	postal horn	5	postal_horn	post
🔔	bell	5	bell	break church sound
🔕	bell with slash	5	no_bell	forbidden mute not prohibited quiet silent sound
🎼	musical score	5	musical_score	music note
🎵	musical note	5	musical_note	music sound
🎶	musical notes	5	notes musical_notes	music note sound
🎙️	studio microphone	5	studio_microphone	mic music
🎚️	level slider	5	level_slider	music
🎛️	control knobs	5	control_knobs	music
🎤	microphone	5	microphone	karaoke mic music sing sound
🎧️	headphone	5	headphones	earbud sound
📻️	radio	5	radio	entertainment tbt video
🎷	saxophone	5	saxophone	instrument music sax
🎺	trumpet	5	trumpet	instrument music
🪊	trombone	5	trombone	brass instrument jazz music sad slide
🪗	accordion	5	accordion	box concertina instrument music squeeze squeezebox
🎸	guitar	5	guitar	instrument music strat
🎹	musical keyboard	5	musical_keyboard	instrument music piano
🎻	violin	5	violin	instrument music
🪕	banjo	5	banjo	music stringed
🥁	drum	5	drum_with_drumsticks drum	music
🪘	long drum	5	long_drum	beat conga instrument rhythm
🪇	maracas	5	maracas	cha dance instrument music party percussion rattle shake shaker
🪈	flute	5	flute	band fife flautist instrument marching music orchestra piccolo pipe recorder woodwind
🪉	harp	5	harp	cupid instrument love music orchestra
📱	mobile phone	5	iphone android mobile_phone	cell communication telephone
📲	mobile phone with arrow	5	calling mobile_phone_arrow	build call cell communication receive telephone
☎️	telephone	5	phone telephone	
📞	telephone receiver	5	telephone_receiver	communication phone voip
📟️	pager	5	pager	communication
📠	fax machine	5	fax fax_machine	communication
🔋	battery	5	battery	
🪫	low battery	5	low_battery	drained electronic energy power
🔌	electric plug	5	electric_plug	electricity
💻️	laptop	5	computer laptop	office pc personal
🖥️	desktop computer	5	desktop_computer	monitor
🖨️	printer	5	printer	computer
⌨️	keyboard	5	keyboard	computer
🖱️	computer mouse	5	three_button_mouse computer_mouse	
🖲️	trackball	5	trackball	computer
💽	computer disk	5	minidisc computer_disk	minidisk optical
💾	floppy disk	5	floppy_disk	computer
💿️	optical disk	5	cd optical_disk	blu-ray computer dvd
📀	dvd	5	dvd	blu-ray cd computer disk optical
🧮	abacus	5	abacus	calculation calculator
🎥	movie camera	5	movie_camera	bollywood cinema film hollywood record
🎞️	film frames	5	film_frames film_strip	cinema movie
📽️	film projector	5	film_projector	cinema movie video
🎬️	clapper board	5	clapper	action movie
📺️	television	5	tv	video
📷️	camera	5	camera	photo selfie snap tbt trip video
📸	camera with flash	5	camera_with_flash camera_flash	video
📹️	video camera	5	video_camera	camcorder tbt
📼	videocassette	5	vhs videocassette	old school tape vcr video
🔍️	magnifying glass tilted left	5	mag	lab left-pointing science search tool
🔎	magnifying glass tilted right	5	mag_right	contact lab right-pointing science search tool
🕯️	candle	5	candle	light
💡	light bulb	5	bulb light_bulb	comic electric idea
🔦	flashlight	5	flashlight	electric light tool torch
🏮	red paper lantern	5	izakaya_lantern lantern red_paper_lantern	bar light restaurant
🪔	diya lamp	5	diya_lamp	light oil
📔	notebook with decorative cover	5	notebook_with_decorative_cover	book decorated education school writing
📕	closed book	5	closed_book	education
📖	open book	5	book open_book	education fantasy knowledge library novels reading
📗	green book	5	green_book	education fantasy library reading
📘	blue book	5	blue_book	education fantasy library reading
📙	orange book	5	orange_book	education fantasy library reading
📚️	books	5	books	book education fantasy knowledge library novels reading school study
📓	notebook	5	notebook	
📒	ledger	5	ledger	notebook
📃	page with curl	5	page_with_curl	document paper
📜	scroll	5	scroll	paper
📄	page facing up	5	page_facing_up	document paper
📰	newspaper	5	newspaper	communication news paper
🗞️	rolled-up newspaper	5	rolled_up_newspaper newspaper_roll	news paper rolled-up
📑	bookmark tabs	5	bookmark_tabs	mark marker
🔖	bookmark	5	bookmark	mark
🏷️	label	5	label	tag
🪙	coin	5	coin	dollar euro gold metal money rich silver treasure
💰️	money bag	5	moneybag	bank bet billion cash cost dollar gold million paid paying pot rich win
🪎	treasure chest	5	treasure_chest	gem gold jewels loot money prize silver valuables wealth
💴	yen banknote	5	yen	bank bill currency money note
💵	dollar banknote	5	dollar	bank bill currency money note
💶	euro banknote	5	euro	100 bank bill currency money note rich
💷	pound banknote	5	pound	bank bill billion cash currency money note pounds
💸	money with wings	5	money_with_wings	bank banknote bill billion cash dollar fly million note pay
💳️	credit card	5	credit_card	bank cash charge money pay
🧾	receipt	5	receipt	accounting bookkeeping evidence invoice proof
💹	chart increasing with yen	5	chart	bank currency graph growth market money rise trend upward
✉️	envelope	5	email envelope	e-mail letter
📧	e-mail	5	e-mail	email letter
📨	incoming envelope	5	incoming_envelope	delivering e-mail email letter mail receive sent
📩	envelope with arrow	5	envelope_with_arrow	communication down e-mail email letter mail outgoing send sent
📤️	outbox tray	5	outbox_tray	box email letter mail sent
📥️	inbox tray	5	inbox_tray	box email letter mail receive zero
📦️	package	5	package	box communication delivery parcel shipping
📫️	closed mailbox with raised flag	5	mailbox	communication mail postbox
📪️	closed mailbox with lowered flag	5	mailbox_closed	mail postbox
📬️	open mailbox with raised flag	5	mailbox_with_mail	postbox
📭️	open mailbox with lowered flag	5	mailbox_with_no_mail	postbox
📮	postbox	5	postbox	mail mailbox
🗳️	ballot box with ballot	5	ballot_box_with_ballot ballot_box	
✏️	pencil	5	pencil2	
✒️	black nib	5	black_nib	pen
🖋️	fountain pen	5	lower_left_fountain_pen fountain_pen	
🖊️	pen	5	lower_left_ballpoint_pen pen	
🖌️	paintbrush	5	lower_left_paintbrush paintbrush	painting
🖍️	crayon	5	lower_left_crayon crayon	
📝	memo	5	memo pencil	communication media notes
💼	briefcase	5	briefcase	office
📁	file folder	5	file_folder	
📂	open file folder	5	open_file_folder	
🗂️	card index dividers	5	card_index_dividers	
📅	calendar	5	date	
📆	tear-off calendar	5	calendar	tear-off
🗒️	spiral notepad	5	spiral_note_pad spiral_notepad notepad_spiral	
🗓️	spiral calendar	5	spiral_calendar_pad spiral_calendar calendar_spiral	
📇	card index	5	card_index	old rolodex school
📈	chart increasing	5	chart_with_upwards_trend chart_increasing	data graph growth right up upward
📉	chart decreasing	5	chart_with_downwards_trend chart_decreasing	data down downward graph negative
📊	bar chart	5	bar_chart	data graph
📋️	clipboard	5	clipboard	do list notes
📌	pushpin	5	pushpin	collage pin
📍	round pushpin	5	round_pushpin	location map pin
📎	paperclip	5	paperclip	
🖇️	linked paperclips	5	linked_paperclips paperclips	link paperclip
📏	straight ruler	5	straight_ruler	angle edge math straightedge
📐	triangular ruler	5	triangular_ruler	angle math rule set slide triangle
✂️	scissors	5	scissors	cut cutting paper tool
🗃️	card file box	5	card_file_box	
🗄️	file cabinet	5	file_cabinet	filing paper
🗑️	wastebasket	5	wastebasket trashcan	can garbage trash waste
🔒️	locked	5	lock locked	closed private
🔓️	unlocked	5	unlock unlocked	cracked lock open
🔏	locked with pen	5	lock_with_ink_pen locked_with_pen	nib privacy
🔐	locked with key	5	closed_lock_with_key locked_with_key	bike secure
🔑	key	5	key	keys lock major password unlock
🗝️	old key	5	old_key	clue lock
🔨	hammer	5	hammer	home improvement repairs tool
🪓	axe	5	axe	ax chop hatchet split wood
⛏️	pick	5	pick	hammer mining tool
⚒️	hammer and pick	5	hammer_and_pick	tool
🛠️	hammer and wrench	5	hammer_and_wrench	spanner tool
🗡️	dagger	5	dagger_knife dagger	weapon
⚔️	crossed swords	5	crossed_swords	weapon
💣️	bomb	5	bomb	boom comic dangerous explosion hot
🪃	boomerang	5	boomerang	rebound repercussion weapon
🏹	bow and arrow	5	bow_and_arrow	archer archery sagittarius tool weapon zodiac
🛡️	shield	5	shield	weapon
🪚	carpentry saw	5	carpentry_saw	carpenter cut lumber tool trim
🔧	wrench	5	wrench	home improvement spanner tool
🪛	screwdriver	5	screwdriver	flathead handy screw tool
🔩	nut and bolt	5	nut_and_bolt	home improvement tool
⚙️	gear	5	gear	cog cogwheel tool
🗜️	clamp	5	compression clamp	compress tool vice
⚖️	balance scale	5	scales balance_scale	justice libra tool weight zodiac
🦯	white cane	5	probing_cane white_cane	accessibility blind
🔗	link	5	link	links
⛓️‍💥	broken chain	5	broken_chain	break breaking cuffs freedom
⛓️	chains	5	chains	chain
🪝	hook	5	hook	catch crook curve ensnare point selling
🧰	toolbox	5	toolbox	box chest mechanic red tool
🧲	magnet	5	magnet	attraction horseshoe magnetic negative positive shape u
🪜	ladder	5	ladder	climb rung step
🪏	shovel	5	shovel	bury dig garden hole plant scoop snow spade
⚗️	alembic	5	alembic	chemistry tool
🧪	test tube	5	test_tube	chemist chemistry experiment lab science
🧫	petri dish	5	petri_dish	bacteria biologist biology culture lab
🧬	dna	5	dna double_helix	biologist evolution gene genetics life
🔬	microscope	5	microscope	experiment lab science tool
🔭	telescope	5	telescope	contact extraterrestrial science tool
📡	satellite antenna	5	satellite_antenna	aliens contact dish science
💉	syringe	5	syringe	doctor flu medicine needle shot sick tool vaccination
🩸	drop of blood	5	drop_of_blood	bleed donation injury medicine menstruation
💊	pill	5	pill	doctor drugs medicated medicine pills sick vitamin
🩹	adhesive bandage	5	adhesive_bandage bandaid	
🩼	crutch	5	crutch	aid cane disability help hurt injured mobility stick
🩺	stethoscope	5	stethoscope	doctor heart medicine
🩻	x-ray	5	x-ray x_ray xray	bones doctor medical skeleton skull
🚪	door	5	door	back closet front
🛗	elevator	5	elevator	accessibility hoist lift
🪞	mirror	5	mirror	makeup reflection reflector speculum
🪟	window	5	window	air frame fresh opening transparent view
🛏️	bed	5	bed	hotel sleep
🛋️	couch and lamp	5	couch_and_lamp	hotel
🪑	chair	5	chair	seat sit
🚽	toilet	5	toilet	bathroom
🪠	plunger	5	plunger	cup force plumber poop suction toilet
🚿	shower	5	shower	water
🛁	bathtub	5	bathtub	bath
🪤	mouse trap	5	mouse_trap	bait cheese lure mousetrap snare
🪒	razor	5	razor	sharp shave
🧴	lotion bottle	5	lotion_bottle	moisturizer shampoo sunscreen
🧷	safety pin	5	safety_pin	diaper punk rock
🧹	broom	5	broom	cleaning sweeping witch
🧺	basket	5	basket	farming laundry picnic
🧻	roll of paper	5	roll_of_paper toilet_paper	towels
🪣	bucket	5	bucket	cask pail vat
🧼	soap	5	soap	bar bathing clean cleaning lather soapdish
🫧	bubbles	5	bubbles	bubble burp clean floating pearl soap underwater
🪥	toothbrush	5	toothbrush	bathroom brush clean dental hygiene teeth toiletry
🧽	sponge	5	sponge	absorbing cleaning porous soak
🧯	fire extinguisher	5	fire_extinguisher	extinguish quench
🛒	shopping cart	5	shopping_trolley shopping_cart	
🚬	cigarette	5	smoking cigarette	
⚰️	coffin	5	coffin	dead death vampire
🪦	headstone	5	headstone	cemetery dead grave graveyard memorial rip tomb tombstone
⚱️	funeral urn	5	funeral_urn	ashes death
🧿	nazar amulet	5	nazar_amulet	bead blue charm evil-eye talisman
🪬	hamsa	5	hamsa	amulet fatima fortune guide hand mary miriam palm protect protection
🗿	moai	5	moyai moai	face statue stoneface travel
🪧	placard	5	placard	card demonstration notice picket plaque protest sign
🪪	identification card	5	identification_card id_card	credentials document license security
🏧	atm sign	6	atm	automated bank cash money teller
🚮	litter in bin sign	6	put_litter_in_its_place litter_bin	litterbin
🚰	potable water	6	potable_water	drinking
♿️	wheelchair symbol	6	wheelchair handicapped	access handicap
🚹️	men’s room	6	mens	bathroom lavatory man men’s restroom toilet wc
🚺️	women’s room	6	womens	bathroom lavatory restroom toilet wc woman women’s
🚻	restroom	6	restroom bathroom	lavatory toilet wc
🚼️	baby symbol	6	baby_symbol	changing
🚾	water closet	6	wc water_closet	bathroom lavatory restroom toilet
🛂	passport control	6	passport_control	
🛃	customs	6	customs	packing
🛄	baggage claim	6	baggage_claim	arrived bags case checked journey packing plane ready travel trip
🛅	left luggage	6	left_luggage	baggage case locker
⚠️	warning	6	warning	caution
🚸	children crossing	6	children_crossing	child pedestrian traffic
⛔️	no entry	6	no_entry	do fail forbidden not pass prohibited traffic
🚫	prohibited	6	no_entry_sign	forbidden not smoke
🚳	no bicycles	6	no_bicycles	bicycle bike forbidden not prohibited
🚭️	no smoking	6	no_smoking	forbidden not prohibited smoke
🚯	no littering	6	do_not_litter no_littering	forbidden prohibited
🚱	non-potable water	6	non-potable_water	dry non-drinking non-potable prohibited
🚷	no pedestrians	6	no_pedestrians	forbidden not pedestrian prohibited
📵	no mobile phones	6	no_mobile_phones	cell forbidden not phone prohibited telephone
🔞	no one under eighteen	6	underage no_one_under_18	age forbidden not prohibited restriction
☢️	radioactive	6	radioactive_sign radioactive	
☣️	biohazard	6	biohazard_sign biohazard	
⬆️	up arrow	6	arrow_up	cardinal direction north
↗️	up-right arrow	6	arrow_upper_right	direction intercardinal northeast up-right
➡️	right arrow	6	arrow_right	cardinal direction east
↘️	down-right arrow	6	arrow_lower_right	direction down-right intercardinal southeast
⬇️	down arrow	6	arrow_down	cardinal direction south
↙️	down-left arrow	6	arrow_lower_left	direction down-left intercardinal southwest
⬅️	left arrow	6	arrow_left	cardinal direction west
↖️	up-left arrow	6	arrow_upper_left	direction intercardinal northwest up-left
↕️	up-down arrow	6	arrow_up_down	up-down
↔️	left-right arrow	6	left_right_arrow	left-right
↩️	right arrow curving left	6	leftwards_arrow_with_hook arrow_left_hook	
↪️	left arrow curving right	6	arrow_right_hook rightwards_arrow_with_hook	
⤴️	right arrow curving up	6	arrow_heading_up	
⤵️	right arrow curving down	6	arrow_heading_down	
🔃	clockwise vertical arrows	6	arrows_clockwise clockwise	arrow refresh reload
🔄	counterclockwise arrows button	6	arrows_counterclockwise counterclockwise	again anticlockwise arrow deja refresh rewindershins vu
🔙	back arrow	6	back	
🔚	end arrow	6	end	
🔛	on! arrow	6	on	mark on!
🔜	soon arrow	6	soon	brb omw
🔝	top arrow	6	top	homie up
🛐	place of worship	6	place_of_worship	pray religion
⚛️	atom symbol	6	atom_symbol atom	atheist
🕉️	om	6	om_symbol om	hindu religion
✡️	star of david	6	star_of_david	jew jewish judaism religion
☸️	wheel of dharma	6	wheel_of_dharma	buddhist religion
☯️	yin yang	6	yin_yang	difficult lives religion tao taoist total yinyang
✝️	latin cross	6	latin_cross	christ christian religion
☦️	orthodox cross	6	orthodox_cross	christian religion
☪️	star and crescent	6	star_and_crescent	islam muslim ramadan religion
☮️	peace symbol	6	peace_symbol peace	healing peaceful
🕎	menorah	6	menorah_with_nine_branches menorah	candelabrum candlestick hanukkah jewish judaism religion
🔯	dotted six-pointed star	6	six_pointed_star	fortune jewish judaism six-pointed
🪯	khanda	6	khanda	deg fateh khalsa religion sikh sikhism tegh
♈️	aries	6	aries	horoscope ram zodiac
♉️	taurus	6	taurus	bull horoscope ox zodiac
♊️	gemini	6	gemini	horoscope twins zodiac
♋️	cancer	6	cancer	crab horoscope zodiac
♌️	leo	6	leo	horoscope lion zodiac
♍️	virgo	6	virgo	horoscope zodiac
♎️	libra	6	libra	balance horoscope justice scales zodiac
♏️	scorpio	6	scorpius	horoscope scorpion zodiac
♐️	sagittarius	6	sagittarius	archer horoscope zodiac
♑️	capricorn	6	capricorn	goat horoscope zodiac
♒️	aquarius	6	aquarius	bearer horoscope water zodiac
♓️	pisces	6	pisces	fish horoscope zodiac
⛎️	ophiuchus	6	ophiuchus	bearer serpent snake zodiac
🔀	shuffle tracks button	6	twisted_rightwards_arrows shuffle	arrow crossed
🔁	repeat button	6	repeat	arrow clockwise
🔂	repeat single button	6	repeat_one	arrow clockwise once
▶️	play button	6	arrow_forward play	right triangle
⏩️	fast-forward button	6	fast_forward	arrow double fast-forward
⏭️	next track button	6	black_right_pointing_double_triangle_with_vertical_bar next_track_button next_track	arrow scene
⏯️	play or pause button	6	black_right_pointing_triangle_with_double_vertical_bar play_or_pause_button play_pause	arrow
◀️	reverse button	6	arrow_backward reverse	left triangle
⏪️	fast reverse button	6	rewind fast_reverse	arrow double
⏮️	last track button	6	black_left_pointing_double_triangle_with_vertical_bar previous_track_button previous_track	arrow scene
🔼	upwards button	6	arrow_up_small	red
⏫️	fast up button	6	arrow_double_up fast_up	
🔽	downwards button	6	arrow_down_small down	red
⏬️	fast down button	6	arrow_double_down fast_down	
⏸️	pause button	6	double_vertical_bar pause_button pause	
⏹️	stop button	6	black_square_for_stop stop_button stop	
⏺️	record button	6	black_circle_for_record record_button record	
⏏️	eject button	6	eject eject_button	
🎦	cinema	6	cinema	camera film movie
🔅	dim button	6	low_brightness dim_button	
🔆	bright button	6	high_brightness bright_button	light
📶	antenna bars	6	signal_strength antenna_bars	bar cell communication mobile phone telephone
🛜	wireless	6	wireless	broadband computer connectivity hotspot internet network router smartphone wi-fi wifi wlan
📳	vibration mode	6	vibration_mode	cell communication mobile phone telephone
📴	mobile phone off	6	mobile_phone_off	cell telephone
♀️	female sign	6	female_sign female	woman
♂️	male sign	6	male_sign male	man
⚧️	transgender symbol	6	transgender_symbol	
✖️	multiply	6	heavy_multiplication_x multiplication multiply	cancel sign ×
➕️	plus	6	heavy_plus_sign plus	+
➖️	minus	6	heavy_minus_sign minus	- math −
➗️	divide	6	heavy_division_sign divide division	math ÷
🟰	heavy equals sign	6	heavy_equals_sign	answer equal equality math
♾️	infinity	6	infinity	forever unbounded universal
‼️	double exclamation mark	6	bangbang double_exclamation	! !! punctuation
⁉️	exclamation question mark	6	interrobang exclamation_question	! !? ? punctuation
❓️	red question mark	6	question	? punctuation
❔️	white question mark	6	grey_question white_question	? outlined punctuation
❕️	white exclamation mark	6	grey_exclamation white_exclamation	! outlined punctuation
❗️	red exclamation mark	6	exclamation heavy_exclamation_mark	! punctuation
〰️	wavy dash	6	wavy_dash	punctuation
💱	currency exchange	6	currency_exchange	bank money
💲	heavy dollar sign	6	heavy_dollar_sign	billion cash charge currency million money pay
⚕️	medical symbol	6	medical_symbol staff_of_aesculapius medical	medicine
♻️	recycling symbol	6	recycle recycling_symbol	
⚜️	fleur-de-lis	6	fleur_de_lis fleur-de-lis	knights
🔱	trident emblem	6	trident	anchor poseidon ship tool
📛	name badge	6	name_badge	
🔰	japanese symbol for beginner	6	beginner	chevron green leaf tool yellow
⭕️	hollow red circle	6	o hollow_red_circle red_o	heavy large
✅️	check mark button	6	white_check_mark check_mark_button	checked checkmark complete completed done fixed tick ✓
☑️	check box with check	6	ballot_box_with_check	checked done off tick ✓
✔️	check mark	6	heavy_check_mark check_mark	checked checkmark done tick ✓
❌️	cross mark	6	x cross_mark	cancel multiplication multiply ×
❎️	cross mark button	6	negative_squared_cross_mark cross_mark_button	multiplication multiply square x ×
➰️	curly loop	6	curly_loop	curl
➿️	double curly loop	6	loop double_curly_loop	curl
〽️	part alternation mark	6	part_alternation_mark	
✳️	eight-spoked asterisk	6	eight_spoked_asterisk	* eight-spoked
✴️	eight-pointed star	6	eight_pointed_black_star	* eight-pointed
❇️	sparkle	6	sparkle	*
©️	copyright	6	copyright	c
®️	registered	6	registered	r
™️	trade mark	6	tm trade_mark	trademark
🫟	splatter	6	splatter	drip holi ink liquid mess paint spill stain
#️⃣	keycap: #	6	hash number_sign	
*️⃣	keycap: *	6	keycap_star asterisk	
0️⃣	keycap: 0	6	zero	
1️⃣	keycap: 1	6	one	
2️⃣	keycap: 2	6	two	
3️⃣	keycap: 3	6	three	
4️⃣	keycap: 4	6	four	
5️⃣	keycap: 5	6	five	
6️⃣	keycap: 6	6	six	
7️⃣	keycap: 7	6	seven	
8️⃣	keycap: 8	6	eight	
9️⃣	keycap: 9	6	nine	
🔟	keycap: 10	6	keycap_ten ten	
🔠	input latin uppercase	6	capital_abcd	letters
🔡	input latin lowercase	6	abcd	letters
🔢	input numbers	6	1234	
🔣	input symbols	6	symbols	% & ♪ 〒
🔤	input latin letters	6	abc	alphabet
🅰️	a button (blood type)	6	a a_blood	
🆎	ab button (blood type)	6	ab ab_blood	
🅱️	b button (blood type)	6	b b_blood	
🆑	cl button	6	cl	
🆒	cool button	6	cool	
🆓	free button	6	free	
ℹ️	information	6	information_source info	i
🆔	id button	6	id	identity
Ⓜ️	circled m	6	m	circle
🆕	new button	6	new	
🆖	ng button	6	ng	
🅾️	o button (blood type)	6	o2 o_blood	
🆗	ok button	6	ok	okay
🅿️	p button	6	parking	
🆘	sos button	6	sos	help
🆙	up! button	6	up up2	mark up!
🆚	vs button	6	vs	versus
🈁	japanese “here” button	6	koko ja_here	katakana
🈂️	japanese “service charge” button	6	sa ja_service_charge	katakana
🈷️	japanese “monthly amount” button	6	u6708 ja_monthly_amount	ideograph
🈶	japanese “not free of charge” button	6	u6709 ja_not_free_of_carge	ideograph
🈯️	japanese “reserved” button	6	u6307 ja_reserved	ideograph
🉐	japanese “bargain” button	6	ideograph_advantage ja_bargain	
🈹	japanese “discount” button	6	u5272 ja_discount	ideograph
🈚️	japanese “free of charge” button	6	u7121 ja_free_of_charge	ideograph
🈲	japanese “prohibited” button	6	u7981 ja_prohibited	ideograph
🉑	japanese “acceptable” button	6	accept ja_acceptable	ideograph
🈸	japanese “application” button	6	u7533 ja_application	ideograph
🈴	japanese “passing grade” button	6	u5408 ja_passing_grade	ideograph
🈳	japanese “vacancy” button	6	u7a7a ja_vacancy	ideograph
㊗️	japanese “congratulations” button	6	congratulations ja_congratulations	ideograph
㊙️	japanese “secret” button	6	secret ja_secret	ideograph
🈺	japanese “open for business” button	6	u55b6 ja_open_for_business	ideograph
🈵	japanese “no vacancy” button	6	u6e80 ja_no_vacancy	ideograph
🔴	red circle	6	red_circle	geometric
🟠	orange circle	6	large_orange_circle orange_circle	
🟡	yellow circle	6	large_yellow_circle yellow_circle	
🟢	green circle	6	large_green_circle green_circle	
🔵	blue circle	6	large_blue_circle blue_circle	geometric
🟣	purple circle	6	large_purple_circle purple_circle	
🟤	brown circle	6	large_brown_circle brown_circle	
⚫️	black circle	6	black_circle	geometric
⚪️	white circle	6	white_circle	geometric
🟥	red square	6	large_red_square red_square	card penalty
🟧	orange square	6	large_orange_square orange_square	
🟨	yellow square	6	large_yellow_square yellow_square	card penalty
🟩	green square	6	large_green_square green_square	
🟦	blue square	6	large_blue_square blue_square	
🟪	purple square	6	large_purple_square purple_square	
🟫	brown square	6	large_brown_square brown_square	
⬛️	black large square	6	black_large_square	geometric
⬜️	white large square	6	white_large_square	geometric
◼️	black medium square	6	black_medium_square	geometric
◻️	white medium square	6	white_medium_square	geometric
◾️	black medium-small square	6	black_medium_small_square	geometric medium-small
◽️	white medium-small square	6	white_medium_small_square	geometric medium-small
▪️	black small square	6	black_small_square	geometric
▫️	white small square	6	white_small_square	geometric
🔶	large orange diamond	6	large_orange_diamond	geometric
🔷	large blue diamond	6	large_blue_diamond	geometric
🔸	small orange diamond	6	small_orange_diamond	geometric
🔹	small blue diamond	6	small_blue_diamond	geometric
🔺	red triangle pointed up	6	small_red_triangle	geometric
🔻	red triangle pointed down	6	small_red_triangle_down	geometric
💠	diamond with a dot	6	diamond_shape_with_a_dot_inside diamond_with_a_dot	comic geometric
🔘	radio button	6	radio_button	geometric
🔳	white square button	6	white_square_button	geometric outlined
🔲	black square button	6	black_square_button	geometric
🏁	chequered flag	7	checkered_flag	finish flags game race racing sport win
🚩	triangular flag	7	triangular_flag_on_post triangular_flag	construction golf
🎌	crossed flags	7	crossed_flags	celebration cross japanese
🏴	black flag	7	waving_black_flag black_flag	
🏳️	white flag	7	waving_white_flag white_flag	
🏳️‍🌈	rainbow flag	7	rainbow-flag rainbow_flag	bisexual gay genderqueer glbt glbtq lesbian lgbt lgbtq lgbtqia pride queer trans transgender
🏳️‍⚧️	transgender flag	7	transgender_flag	blue light pink white
🏴‍☠️	pirate flag	7	pirate_flag jolly_roger	plunder treasure
🇦🇨	flag: ascension island	7	flag-ac ascension_island flag_ac	
🇦🇩	flag: andorra	7	flag-ad andorra flag_ad	
🇦🇪	flag: united arab emirates	7	flag-ae united_arab_emirates flag_ae	
🇦🇫	flag: afghanistan	7	flag-af afghanistan flag_af	
🇦🇬	flag: antigua & barbuda	7	flag-ag antigua_barbuda flag_ag	
🇦🇮	flag: anguilla	7	flag-ai anguilla flag_ai	
🇦🇱	flag: albania	7	flag-al albania flag_al	
🇦🇲	flag: armenia	7	flag-am armenia flag_am	
🇦🇴	flag: angola	7	flag-ao angola flag_ao	
🇦🇶	flag: antarctica	7	flag-aq antarctica flag_aq	
🇦🇷	flag: argentina	7	flag-ar argentina flag_ar	
🇦🇸	flag: american samoa	7	flag-as american_samoa flag_as	
🇦🇹	flag: austria	7	flag-at austria flag_at	
🇦🇺	flag: australia	7	flag-au australia flag_au	
🇦🇼	flag: aruba	7	flag-aw aruba flag_aw	
🇦🇽	flag: åland islands	7	flag-ax aland_islands flag_ax	
🇦🇿	flag: azerbaijan	7	flag-az azerbaijan flag_az	
🇧🇦	flag: bosnia & herzegovina	7	flag-ba bosnia_herzegovina flag_ba	
🇧🇧	flag: barbados	7	flag-bb barbados flag_bb	
🇧🇩	flag: bangladesh	7	flag-bd bangladesh flag_bd	
🇧🇪	flag: belgium	7	flag-be belgium flag_be	
🇧🇫	flag: burkina faso	7	flag-bf burkina_faso flag_bf	
🇧🇬	flag: bulgaria	7	flag-bg bulgaria flag_bg	
🇧🇭	flag: bahrain	7	flag-bh bahrain flag_bh	
🇧🇮	flag: burundi	7	flag-bi burundi flag_bi	
🇧🇯	flag: benin	7	flag-bj benin flag_bj	
🇧🇱	flag: st. barthélemy	7	flag-bl st_barthelemy flag_bl	
🇧🇲	flag: bermuda	7	flag-bm bermuda flag_bm	
🇧🇳	flag: brunei	7	flag-bn brunei flag_bn	
🇧🇴	flag: bolivia	7	flag-bo bolivia flag_bo	
🇧🇶	flag: caribbean netherlands	7	flag-bq caribbean_netherlands flag_bq	
🇧🇷	flag: brazil	7	flag-br brazil flag_br	
🇧🇸	flag: bahamas	7	flag-bs bahamas flag_bs	
🇧🇹	flag: bhutan	7	flag-bt bhutan flag_bt	
🇧🇻	flag: bouvet island	7	flag-bv bouvet_island flag_bv	
🇧🇼	flag: botswana	7	flag-bw botswana flag_bw	
🇧🇾	flag: belarus	7	flag-by belarus flag_by	
🇧🇿	flag: belize	7	flag-bz belize flag_bz	
🇨🇦	flag: canada	7	flag-ca canada flag_ca	
🇨🇨	flag: cocos (keeling) islands	7	flag-cc cocos_islands flag_cc	
🇨🇩	flag: congo - kinshasa	7	flag-cd congo_kinshasa flag_cd	
🇨🇫	flag: central african republic	7	flag-cf central_african_republic flag_cf	
🇨🇬	flag: congo - brazzaville	7	flag-cg congo_brazzaville flag_cg	
🇨🇭	flag: switzerland	7	flag-ch switzerland flag_ch	
🇨🇮	flag: côte d’ivoire	7	flag-ci cote_divoire flag_ci	
🇨🇰	flag: cook islands	7	flag-ck cook_islands flag_ck	
🇨🇱	flag: chile	7	flag-cl chile flag_cl	
🇨🇲	flag: cameroon	7	flag-cm cameroon flag_cm	
🇨🇳	flag: china	7	cn flag-cn china flag_cn	
🇨🇴	flag: colombia	7	flag-co colombia flag_co	
🇨🇵	flag: clipperton island	7	flag-cp clipperton_island flag_cp	
🇨🇶	flag: sark	7	flag-sark flag_cq sark	
🇨🇷	flag: costa rica	7	flag-cr costa_rica flag_cr	
🇨🇺	flag: cuba	7	flag-cu cuba flag_cu	
🇨🇻	flag: cape verde	7	flag-cv cape_verde flag_cv	
🇨🇼	flag: curaçao	7	flag-cw curacao flag_cw	
🇨🇽	flag: christmas island	7	flag-cx christmas_island flag_cx	
🇨🇾	flag: cyprus	7	flag-cy cyprus flag_cy	
🇨🇿	flag: czechia	7	flag-cz czech_republic czechia flag_cz	
🇩🇪	flag: germany	7	de flag-de flag_de germany	
🇩🇬	flag: diego garcia	7	flag-dg diego_garcia flag_dg	
🇩🇯	flag: djibouti	7	flag-dj djibouti flag_dj	
🇩🇰	flag: denmark	7	flag-dk denmark flag_dk	
🇩🇲	flag: dominica	7	flag-dm dominica flag_dm	
🇩🇴	flag: dominican republic	7	flag-do dominican_republic flag_do	
🇩🇿	flag: algeria	7	flag-dz algeria flag_dz	
🇪🇦	flag: ceuta & melilla	7	flag-ea ceuta_melilla flag_ea	
🇪🇨	flag: ecuador	7	flag-ec ecuador flag_ec	
🇪🇪	flag: estonia	7	flag-ee estonia flag_ee	
🇪🇬	flag: egypt	7	flag-eg egypt flag_eg	
🇪🇭	flag: western sahara	7	flag-eh western_sahara flag_eh	
🇪🇷	flag: eritrea	7	flag-er eritrea flag_er	
🇪🇸	flag: spain	7	es flag-es flag_es spain	
🇪🇹	flag: ethiopia	7	flag-et ethiopia flag_et	
🇪🇺	flag: european union	7	flag-eu eu european_union flag_eu	
🇫🇮	flag: finland	7	flag-fi finland flag_fi	
🇫🇯	flag: fiji	7	flag-fj fiji flag_fj	
🇫🇰	flag: falkland islands	7	flag-fk falkland_islands flag_fk	
🇫🇲	flag: micronesia	7	flag-fm micronesia flag_fm	
🇫🇴	flag: faroe islands	7	flag-fo faroe_islands flag_fo	
🇫🇷	flag: france	7	fr flag-fr flag_fr france	
🇬🇦	flag: gabon	7	flag-ga gabon flag_ga	
🇬🇧	flag: united kingdom	7	gb uk flag-gb flag_gb united_kingdom	
🇬🇩	flag: grenada	7	flag-gd grenada flag_gd	
🇬🇪	flag: georgia	7	flag-ge georgia flag_ge	
🇬🇫	flag: french guiana	7	flag-gf french_guiana flag_gf	
🇬🇬	flag: guernsey	7	flag-gg guernsey flag_gg	
🇬🇭	flag: ghana	7	flag-gh ghana flag_gh	
🇬🇮	flag: gibraltar	7	flag-gi gibraltar flag_gi	
🇬🇱	flag: greenland	7	flag-gl greenland flag_gl	
🇬🇲	flag: gambia	7	flag-gm gambia flag_gm	
🇬🇳	flag: guinea	7	flag-gn guinea flag_gn	
🇬🇵	flag: guadeloupe	7	flag-gp guadeloupe flag_gp	
🇬🇶	flag: equatorial guinea	7	flag-gq equatorial_guinea flag_gq	
🇬🇷	flag: greece	7	flag-gr greece flag_gr	
🇬🇸	flag: south georgia & south sandwich islands	7	flag-gs south_georgia_south_sandwich_islands flag_gs	
🇬🇹	flag: guatemala	7	flag-gt guatemala flag_gt	
🇬🇺	flag: guam	7	flag-gu guam flag_gu	
🇬🇼	flag: guinea-bissau	7	flag-gw guinea_bissau flag_gw	
🇬🇾	flag: guyana	7	flag-gy guyana flag_gy	
🇭🇰	flag: hong kong sar china	7	flag-hk hong_kong flag_hk	
🇭🇲	flag: heard & mcdonald islands	7	flag-hm heard_mcdonald_islands flag_hm	
🇭🇳	flag: honduras	7	flag-hn honduras flag_hn	
🇭🇷	flag: croatia	7	flag-hr croatia flag_hr	
🇭🇹	flag: haiti	7	flag-ht haiti flag_ht	
🇭🇺	flag: hungary	7	flag-hu hungary flag_hu	
🇮🇨	flag: canary islands	7	flag-ic canary_islands flag_ic	
🇮🇩	flag: indonesia	7	flag-id indonesia flag_id	
🇮🇪	flag: ireland	7	flag-ie ireland flag_ie	
🇮🇱	flag: israel	7	flag-il israel flag_il	
🇮🇲	flag: isle of man	7	flag-im isle_of_man flag_im	
🇮🇳	flag: india	7	flag-in india flag_in	
🇮🇴	flag: british indian ocean territory	7	flag-io british_indian_ocean_territory flag_io	
🇮🇶	flag: iraq	7	flag-iq iraq flag_iq	
🇮🇷	flag: iran	7	flag-ir iran flag_ir	
🇮🇸	flag: iceland	7	flag-is iceland flag_is	
🇮🇹	flag: italy	7	it flag-it flag_it italy	
🇯🇪	flag: jersey	7	flag-je jersey flag_je	
🇯🇲	flag: jamaica	7	flag-jm jamaica flag_jm	
🇯🇴	flag: jordan	7	flag-jo jordan flag_jo	
🇯🇵	flag: japan	7	jp flag-jp flag_jp	
🇰🇪	flag: kenya	7	flag-ke kenya flag_ke	
🇰🇬	flag: kyrgyzstan	7	flag-kg kyrgyzstan flag_kg	
🇰🇭	flag: cambodia	7	flag-kh cambodia flag_kh	
🇰🇮	flag: kiribati	7	flag-ki kiribati flag_ki	
🇰🇲	flag: comoros	7	flag-km comoros flag_km	
🇰🇳	flag: st. kitts & nevis	7	flag-kn st_kitts_nevis flag_kn	
🇰🇵	flag: north korea	7	flag-kp north_korea flag_kp	
🇰🇷	flag: south korea	7	kr flag-kr flag_kr south_korea	
🇰🇼	flag: kuwait	7	flag-kw kuwait flag_kw	
🇰🇾	flag: cayman islands	7	flag-ky cayman_islands flag_ky	
🇰🇿	flag: kazakhstan	7	flag-kz kazakhstan flag_kz	
🇱🇦	flag: laos	7	flag-la laos flag_la	
🇱🇧	flag: lebanon	7	flag-lb lebanon flag_lb	
🇱🇨	flag: st. lucia	7	flag-lc st_lucia flag_lc	
🇱🇮	flag: liechtenstein	7	flag-li liechtenstein flag_li	
🇱🇰	flag: sri lanka	7	flag-lk sri_lanka flag_lk	
🇱🇷	flag: liberia	7	flag-lr liberia flag_lr	
🇱🇸	flag: lesotho	7	flag-ls lesotho flag_ls	
🇱🇹	flag: lithuania	7	flag-lt lithuania flag_lt	
🇱🇺	flag: luxembourg	7	flag-lu luxembourg flag_lu	
🇱🇻	flag: latvia	7	flag-lv latvia flag_lv	
🇱🇾	flag: libya	7	flag-ly libya flag_ly	
🇲🇦	flag: morocco	7	flag-ma morocco flag_ma	
🇲🇨	flag: monaco	7	flag-mc monaco flag_mc	
🇲🇩	flag: moldova	7	flag-md moldova flag_md	
🇲🇪	flag: montenegro	7	flag-me montenegro flag_me	
🇲🇫	flag: st. martin	7	flag-mf st_martin flag_mf	
🇲🇬	flag: madagascar	7	flag-mg madagascar flag_mg	
🇲🇭	flag: marshall islands	7	flag-mh marshall_islands flag_mh	
🇲🇰	flag: north macedonia	7	flag-mk macedonia flag_mk	
🇲🇱	flag: mali	7	flag-ml mali flag_ml	
🇲🇲	flag: myanmar (burma)	7	flag-mm myanmar burma flag_mm	
🇲🇳	flag: mongolia	7	flag-mn mongolia flag_mn	
🇲🇴	flag: macao sar china	7	flag-mo macau flag_mo macao	
🇲🇵	flag: northern mariana islands	7	flag-mp northern_mariana_islands flag_mp	
🇲🇶	flag: martinique	7	flag-mq martinique flag_mq	
🇲🇷	flag: mauritania	7	flag-mr mauritania flag_mr	
🇲🇸	flag: montserrat	7	flag-ms montserrat flag_ms	
🇲🇹	flag: malta	7	flag-mt malta flag_mt	
🇲🇺	flag: mauritius	7	flag-mu mauritius flag_mu	
🇲🇻	flag: maldives	7	flag-mv maldives flag_mv	
🇲🇼	flag: malawi	7	flag-mw malawi flag_mw	
🇲🇽	flag: mexico	7	flag-mx mexico flag_mx	
🇲🇾	flag: malaysia	7	flag-my malaysia flag_my	
🇲🇿	flag: mozambique	7	flag-mz mozambique flag_mz	
🇳🇦	flag: namibia	7	flag-na namibia flag_na	
🇳🇨	flag: new caledonia	7	flag-nc new_caledonia flag_nc	
🇳🇪	flag: niger	7	flag-ne niger flag_ne	
🇳🇫	flag: norfolk island	7	flag-nf norfolk_island flag_nf	
🇳🇬	flag: nigeria	7	flag-ng nigeria flag_ng	
🇳🇮	flag: nicaragua	7	flag-ni nicaragua flag_ni	
🇳🇱	flag: netherlands	7	flag-nl netherlands flag_nl	
🇳🇴	flag: norway	7	flag-no norway flag_no	
🇳🇵	flag: nepal	7	flag-np nepal flag_np	
🇳🇷	flag: nauru	7	flag-nr nauru flag_nr	
🇳🇺	flag: niue	7	flag-nu niue flag_nu	
🇳🇿	flag: new zealand	7	flag-nz new_zealand flag_nz	
🇴🇲	flag: oman	7	flag-om oman flag_om	
🇵🇦	flag: panama	7	flag-pa panama flag_pa	
🇵🇪	flag: peru	7	flag-pe peru flag_pe	
🇵🇫	flag: french polynesia	7	flag-pf french_polynesia flag_pf	
🇵🇬	flag: papua new guinea	7	flag-pg papua_new_guinea flag_pg	
🇵🇭	flag: philippines	7	flag-ph philippines flag_ph	
🇵🇰	flag: pakistan	7	flag-pk pakistan flag_pk	
🇵🇱	flag: poland	7	flag-pl poland flag_pl	
🇵🇲	flag: st. pierre & miquelon	7	flag-pm st_pierre_miquelon flag_pm	
🇵🇳	flag: pitcairn islands	7	flag-pn pitcairn_islands flag_pn	
🇵🇷	flag: puerto rico	7	flag-pr puerto_rico flag_pr	
🇵🇸	flag: palestinian territories	7	flag-ps palestinian_territories flag_ps	
🇵🇹	flag: portugal	7	flag-pt portugal flag_pt	
🇵🇼	flag: palau	7	flag-pw palau flag_pw	
🇵🇾	flag: paraguay	7	flag-py paraguay flag_py	
🇶🇦	flag: qatar	7	flag-qa qatar flag_qa	
🇷🇪	flag: réunion	7	flag-re reunion flag_re	
🇷🇴	flag: romania	7	flag-ro romania flag_ro	
🇷🇸	flag: serbia	7	flag-rs serbia flag_rs	
🇷🇺	flag: russia	7	ru flag-ru flag_ru russia	
🇷🇼	flag: rwanda	7	flag-rw rwanda flag_rw	
🇸🇦	flag: saudi arabia	7	flag-sa saudi_arabia flag_sa	
🇸🇧	flag: solomon islands	7	flag-sb solomon_islands flag_sb	
🇸🇨	flag: seychelles	7	flag-sc seychelles flag_sc	
🇸🇩	flag: sudan	7	flag-sd sudan flag_sd	
🇸🇪	flag: sweden	7	flag-se sweden flag_se	
🇸🇬	flag: singapore	7	flag-sg singapore flag_sg	
🇸🇭	flag: st. helena	7	flag-sh st_helena flag_sh	
🇸🇮	flag: slovenia	7	flag-si slovenia flag_si	
🇸🇯	flag: svalbard & jan mayen	7	flag-sj svalbard_jan_mayen flag_sj	
🇸🇰	flag: slovakia	7	flag-sk slovakia flag_sk	
🇸🇱	flag: sierra leone	7	flag-sl sierra_leone flag_sl	
🇸🇲	flag: san marino	7	flag-sm san_marino flag_sm	
🇸🇳	flag: senegal	7	flag-sn senegal flag_sn	
🇸🇴	flag: somalia	7	flag-so somalia flag_so	
🇸🇷	flag: suriname	7	flag-sr suriname flag_sr	
🇸🇸	flag: south sudan	7	flag-ss south_sudan flag_ss	
🇸🇹	flag: são tomé & príncipe	7	flag-st sao_tome_principe flag_st	
🇸🇻	flag: el salvador	7	flag-sv el_salvador flag_sv	
🇸🇽	flag: sint maarten	7	flag-sx sint_maarten flag_sx	
🇸🇾	flag: syria	7	flag-sy syria flag_sy	
🇸🇿	flag: eswatini	7	flag-sz swaziland eswatini flag_sz	
🇹🇦	flag: tristan da cunha	7	flag-ta tristan_da_cunha flag_ta	
🇹🇨	flag: turks & caicos islands	7	flag-tc turks_caicos_islands flag_tc	
🇹🇩	flag: chad	7	flag-td chad flag_td	
🇹🇫	flag: french southern territories	7	flag-tf french_southern_territories flag_tf	
🇹🇬	flag: togo	7	flag-tg togo flag_tg	
🇹🇭	flag: thailand	7	flag-th thailand flag_th	
🇹🇯	flag: tajikistan	7	flag-tj tajikistan flag_tj	
🇹🇰	flag: tokelau	7	flag-tk tokelau flag_tk	
🇹🇱	flag: timor-leste	7	flag-tl timor_leste flag_tl	
🇹🇲	flag: turkmenistan	7	flag-tm turkmenistan flag_tm	
🇹🇳	flag: tunisia	7	flag-tn tunisia flag_tn	
🇹🇴	flag: tonga	7	flag-to tonga flag_to	
🇹🇷	flag: türkiye	7	flag-tr tr flag_tr turkey_tr	
🇹🇹	flag: trinidad & tobago	7	flag-tt trinidad_tobago flag_tt	
🇹🇻	flag: tuvalu	7	flag-tv tuvalu flag_tv	
🇹🇼	flag: taiwan	7	flag-tw taiwan flag_tw	
🇹🇿	flag: tanzania	7	flag-tz tanzania flag_tz	
🇺🇦	flag: ukraine	7	flag-ua ukraine flag_ua	
🇺🇬	flag: uganda	7	flag-ug uganda flag_ug	
🇺🇲	flag: u.s. outlying islands	7	flag-um us_outlying_islands flag_um	
🇺🇳	flag: united nations	7	flag-un united_nations flag_un un	
🇺🇸	flag: united states	7	us flag-us flag_us united_states usa	
🇺🇾	flag: uruguay	7	flag-uy uruguay flag_uy	
🇺🇿	flag: uzbekistan	7	flag-uz uzbekistan flag_uz	
🇻🇦	flag: vatican city	7	flag-va vatican_city flag_va	
🇻🇨	flag: st. vincent & grenadines	7	flag-vc st_vincent_grenadines flag_vc	
🇻🇪	flag: venezuela	7	flag-ve venezuela flag_ve	
🇻🇬	flag: british virgin islands	7	flag-vg british_virgin_islands flag_vg	
🇻🇮	flag: u.s. virgin islands	7	flag-vi us_virgin_islands flag_vi	
🇻🇳	flag: vietnam	7	flag-vn vietnam flag_vn	
🇻🇺	flag: vanuatu	7	flag-vu vanuatu flag_vu	
🇼🇫	flag: wallis & futuna	7	flag-wf wallis_futuna flag_wf	
🇼🇸	flag: samoa	7	flag-ws samoa flag_ws	
🇽🇰	flag: kosovo	7	flag-xk kosovo flag_xk	
🇾🇪	flag: yemen	7	flag-ye yemen flag_ye	
🇾🇹	flag: mayotte	7	flag-yt mayotte flag_yt	
🇿🇦	flag: south africa	7	flag-za south_africa flag_za	
🇿🇲	flag: zambia	7	flag-zm zambia flag_zm	
🇿🇼	flag: zimbabwe	7	flag-zw zimbabwe flag_zw	
🏴󠁧󠁢󠁥󠁮󠁧󠁿	flag: england	7	flag-england england flag_gbeng	
🏴󠁧󠁢󠁳󠁣󠁴󠁿	flag: scotland	7	flag-scotland scotland flag_gbsct	
🏴󠁧󠁢󠁷󠁬󠁳󠁿	flag: wales	7	flag-wales wales flag_gbwls	`;

function unpack(): readonly Emoji[] {
  const emoji: Emoji[] = [];
  for (const line of PACKED.split('\n')) {
    const [char, label, category, shortcodes, keywords] = line.split('\t');
    emoji.push({
      char,
      label,
      category: Number(category),
      shortcodes: shortcodes.split(' '),
      keywords: keywords ? keywords.split(' ') : [],
    });
  }
  return emoji;
}

/** Every emoji we know, in Unicode's own order. */
export const EMOJI: readonly Emoji[] = unpack();
