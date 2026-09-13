# 10. Websites aur web apps (LiPi UI)

**Is chapter me seekhoge:** LiPi se website banana: pages, buttons, text box, state (yaad rakhna), components, styling (hover, mobile), aur navbar.

---

## Pehli website

`app.lipi`:

```lipi
page "/"
    heading "Meri pehli website", level: 1
    text "Ye LiPi se bani hai!"
```

Chalao:

```
lipi dev app.lipi
```

Browser me **http://localhost:3000** kholo. File save karo, page apne aap update hota hai. Rokne ke liye **Ctrl + C**.

`page "/"` ek page hai. Andar ke `heading`, `text` page pe dikhne wali cheezein (elements) hain.

---

## Elements

```lipi
page "/"
    heading "Profile", level: 1
    text "Naam: Asha"
    card
        heading "About", level: 3
        text "Main LiPi seekh rahi hoon."
    row
        button "Like"
        button "Share"
    link "LiPi GitHub", to: "https://github.com/iamharshpalsingh/lipi"
    image "photo.png", alt: "Meri photo"
```

| Element | Kya hai |
|---|---|
| `heading "..."` | badi heading (`level: 1` se `6`) |
| `text "..."` | paragraph |
| `card` + block | box (andar ki cheezein) |
| `row` + block | cheezein ek line me |
| `column` + block | cheezein upar-neeche |
| `button "..."` + block | button (block click pe chalta hai) |
| `link "...", to: "/page"` | doosre page ya website ka link |
| `image "file.png"` | photo |
| `field value` | text box (`lines: 4` se lamba box) |
| `checkbox checked, "label"` | tick box |
| `select value, options: [...]` | dropdown |
| `upload "label"` | file / photo chunna |

---

## state: page ko cheezein yaad rakhwana

`state` wala variable badalte hi page apne aap update hota hai:

```lipi
state count = 0

page "/"
    heading "Counter", level: 1
    text "Button {count} baar dabaya"
    row
        button "+1"
            count += 1
        button "Reset", disabled: count == 0
            count = 0
```

`button` ke neeche indent kiya hua code **click pe** chalta hai. `disabled: count == 0` matlab jab count 0 ho to button band.

---

## Text box (field)

```lipi
state naam = ""

page "/"
    field naam, placeholder: "Apna naam likho" with value
        naam = value
    if naam != ""
        heading "Namaste, {naam}!"
```

`with value` me user ka likha hua text aata hai. Har letter pe page update hota hai.

Lamba box chahiye to `lines:` do:

```lipi fragment
field pata, lines: 4, placeholder: "Poora pata" with value
    pata = value
```

---

## Dropdown (select)

```lipi
state shehar = "pune"

page "/"
    select shehar, options: ["delhi", "pune", "kochi"] with chosen
        shehar = chosen
    text "Shehar: {shehar}"
```

Label alag rakhna ho to option ko Object banao:

```lipi fragment
select shehar, options: [{value: "pune", label: "Pune"}] with chosen
    shehar = chosen
```

---

## File ya photo lena (upload)

```lipi
state photos = []

page "/"
    upload "Photo chuno", accept: "image/*", multiple: true with files
        for f in files
            photos.push(f)
    for p in photos
        card
            image p.dataUrl, alt: p.name
            text "{p.name} — {p.size} bytes"
```

Har file ek Object hai: `name`, `type`, `size`, `dataUrl` (seedha `image` me
daal sakte ho) aur `text` (text/JSON/CSV file ka content, warna `null`). Block
tab chalta hai jab file padh li jaati hai.

---

## Poori card ko clickable banana (action)

`button` sirf text leta hai. Jab pura card ya row tap karna ho, `action:` do:

```lipi
state chuna = "kuch nahi"
const brands = [{id: "redmi", name: "Redmi"}, {id: "vivo", name: "Vivo"}]

page "/"
    text "Chuna: {chuna}"
    for b in brands
        card action: () => pick(b.id)
            heading b.name, level: 3
            text "tap karo"

pick(id)
    chuna = id
```

LiPi khud `role="button"` aur `tabindex="0"` laga deta hai, isliye keyboard se
Tab + Enter bhi chalta hai — jo log mouse use nahi karte unke liye zaroori hai.

---

## Components: apne elements banana

Jo cheez baar-baar chahiye, uska component banao:

```lipi
component ProductCard(naam, price)
    card
        heading naam, level: 3
        text "Price: Rs {price}"
        button "Cart me daalo"

page "/"
    heading "Dukaan", level: 1
    ProductCard("Pen", 10)
    ProductCard("Copy", 40)
    ProductCard("Bag", 700)
```

List ke saath:

```lipi
products = [
    {naam: "Pen", price: 10},
    {naam: "Copy", price: 40},
]

component ProductCard(p)
    card
        heading p.naam, level: 3
        text "Rs {p.price}"

page "/"
    for p in products
        ProductCard(p, key: p.naam)
```

`key:` har component ko pehchaan deta hai, taaki list badle to har card ki state sahi item ke saath rahe.

---

## Kai pages aur links

```lipi
page "/"
    heading "Home", level: 1
    link "About page pe jao", to: "/about"

page "/about"
    heading "About", level: 1
    link "Wapas Home", to: "/"

page "/user/:id" with url
    heading "User number {url.params.id}"
```

`/user/:id` me `:id` kuch bhi ho sakta hai: `/user/7` pe `url.params.id` = "7". Code se page badalna ho to `navigate("/about")`.

---

## Styling

Har element ko `style:` do. Andar `property: value;` likho:

```lipi
page "/"
    heading "Sundar heading", style: "color: #D2452A; font-size: 42px;"
    text "Grey text", style: "color: gray; font-style: italic;"
    button "Gol button", style: "border-radius: 999px; padding: 12px 28px;"
```

Style ko ek baar naam do, baar-baar use karo:

```lipi
const cardStyle = "background: white; padding: 20px; border-radius: 16px; box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);"

page "/"
    card style: cardStyle
        text "Pehla card"
    card style: cardStyle
        text "Doosra card"
```

**Hover aur mobile** (LiPi 1.1 se):

```lipi
page "/"
    button "Save", style: "background: green;", hover: "background: darkgreen;"
    text "Sirf computer pe dikhega", mobile: "display: none;"
    text "Sirf phone pe dikhega", desktop: "display: none;"
```

| Option | Kab lagta hai |
|---|---|
| `style:` | hamesha |
| `hover:` | jab mouse upar ho |
| `focus:` | jab keyboard se select ho |
| `mobile:` | phone (720px ya chhoti screen) |
| `desktop:` | computer (badi screen) |

Kuch kaam ki style properties: `color`, `background`, `font-size`, `font-weight`, `padding`, `margin`, `border-radius`, `border`, `width`, `text-align`, `box-shadow`, `display: none`.

---

## Poora navbar (responsive)

Computer pe links dikhenge, phone pe ☰ menu:

```lipi
state menuOpen = false

const barStyle = "background: #17120E; padding: 10px 16px; border-radius: 16px; justify-content: space-between; flex-wrap: nowrap;"
const linkStyle = "color: #DDDDDD; text-decoration: none; padding: 6px 14px; border-radius: 999px;"
const activeStyle = "color: #17120E; background: white; text-decoration: none; padding: 6px 14px; border-radius: 999px;"

component NavLink(label, path, current)
    link label, to: path, style: activeStyle if current == path else linkStyle, hover: "" if current == path else "background: #333333;"

component Navbar(current)
    row style: barStyle
        link "MySite", to: "/", style: "color: white; font-weight: 800; text-decoration: none;"
        row mobile: "display: none;"
            NavLink("Home", "/", current)
            NavLink("About", "/about", current)
        button "✕" if menuOpen else "☰", desktop: "display: none;"
            menuOpen = not menuOpen
    if menuOpen
        column desktop: "display: none;"
            button "Home"
                menuOpen = false
                navigate("/")
            button "About"
                menuOpen = false
                navigate("/about")

page "/" with url
    Navbar(url.path)
    heading "Home", level: 1

page "/about" with url
    Navbar(url.path)
    heading "About", level: 1
```

---

## Har page ka apna naam aur address (SEO)

Page ko apna naam aur ek line ka description do:

```lipi
page "/price", title: "Price — Fixy", description: "Phone ka exact price, bina login ke."
    heading "Price", level: 1
```

`lipi build` har page ki alag HTML file banata hai — `dist/price/index.html` —
jisme wahi title, description aur Open Graph tags hote hain. Matlab Google aur
WhatsApp ko poori site ka ek hi shell nahi, **wo page** dikhta hai.

```sh
lipi build --site https://fixy.in
```

`--site` dene par canonical address, `sitemap.xml` aur `robots.txt` bhi ban
jaate hain.

Server pe chalte waqt address asli hote hain (`/price`), file se kholne par
`#/price` — dono chalte hain, aap ko kuch karna nahi padta. Jis page ke path me
`:id` jaisa part ho uski apni file nahi banti; aise address ke liye host ko
`index.html` (ya build ka likha hua `404.html`) dena hota hai.

---

## Website ko online daalna

```
lipi build app.lipi
```

`dist` folder banega (`index.html` + `app.js`). Ye folder GitHub Pages, Netlify ya Vercel pe daalo. Link milega jo kisi bhi phone/computer ke browser me chalega.

---

## Khud karo

1. Apni "About me" website banao: naam, photo, hobbies ki list.
2. Ek to-do app: text box + "Add" button, neeche list, har item ke saath "Done" button.
3. Ek quiz: ek sawaal, 3 buttons (options). Sahi pe "Shabash!", galat pe "Phir try karo".
4. Apni website me responsive navbar lagao (Home, About, Contact).

<details>
<summary>Jawab dekho (to-do app)</summary>

```lipi
state todos = []
state draft = ""

component TodoItem(title, index)
    row
        text title
        button "Done"
            todos.removeAt(index)

page "/"
    heading "Mera To-Do", level: 1
    row
        field draft, placeholder: "Kya karna hai?" with value
            draft = value
        button "Add", disabled: draft == ""
            todos.push(draft)
            draft = ""
    if todos.isEmpty()
        text "Sab kaam ho gaye!"
    for i, t in todos
        TodoItem(t, i)
```

</details>

<details>
<summary>Jawab dekho (quiz)</summary>

```lipi
state message = ""

page "/"
    heading "India ki rajdhani kya hai?", level: 2
    row
        button "Mumbai"
            message = "Phir try karo"
        button "New Delhi"
            message = "Shabash!"
        button "Kolkata"
            message = "Phir try karo"
    if message != ""
        text message
```

</details>

---

## Yaad rakho

| Cheez | Example |
|---|---|
| page | `page "/about"` |
| chalana | `lipi dev app.lipi` |
| yaad rakhna | `state count = 0` |
| click | `button "+1"` + indented code |
| text box | `field x with value` |
| component | `component Card(title)` |
| link | `link "Home", to: "/"` |
| style | `style:`, `hover:`, `mobile:`, `desktop:` |
| online | `lipi build app.lipi` → `dist` |

> **Teacher tip:** Students ko sabse zyada maza yahan aata hai. Counter app 5 minute me banwao, phir unhe apni pasand ka color aur text badalne do. Jab screen pe unka apna button dikhta hai, confidence bahut badhta hai.
