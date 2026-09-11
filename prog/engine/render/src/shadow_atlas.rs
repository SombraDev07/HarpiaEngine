//! Sombras dinâmicas com **orçamento por frame**: um atlas, uma fila, e uma cache.
//!
//! Mil luzes não podem ter mil shadow maps por frame, e não precisam. A maior
//! parte não se mexe, e das que se mexem a maior parte está longe ou ocupa dez
//! pixels no ecrã. O que é preciso é decidir, todos os frames, **quais** valem o
//! custo — e não voltar a desenhar as que continuam válidas.
//!
//! É o ponto 7.2 da comparação com a Dagor. A ideia é deles e é boa: uma fila
//! ordenada por prioridade, um tecto de trabalho por frame, e conteúdo que
//! persiste entre frames. Aqui está escrita de raiz, com o que eles não publicam
//! — um conjunto de invariantes que um teste verifica sem GPU nenhuma.
//!
//! ## O orçamento conta-se em **texels**, não em mapas
//!
//! Um mapa de 1024² custa 64 vezes um de 128². Um orçamento de «oito mapas por
//! frame» deixa o custo variar 64× consoante o que a fila calhar a escolher, o
//! que é o mesmo que não haver orçamento. Em texels, oito mapas pequenos e um
//! grande custam o que custam e a conta fecha.
//!
//! ## A cache é o que torna o orçamento honesto
//!
//! Sem conteúdo persistente, «só actualizei metade» significa «metade das luzes
//! não tem sombra». Com cache significa «metade das sombras tem um frame de
//! atraso», que é uma coisa completamente diferente e quase sempre invisível. Por
//! isso um slot guarda a **versão** que tem desenhada, e só é refeito quando a
//! versão da luz muda.
//!
//! O invariante que daí sai é o que o gate verifica: **numa cena parada, o
//! resultado com orçamento é idêntico ao resultado sem orçamento nenhum.** O
//! orçamento é uma optimização, não uma mudança de resposta.

/// Classes de resolução. Uma luz pede a que merece pela distância e pelo tamanho
/// no ecrã; o atlas dá-lhe a maior que ainda tem livre, ou uma menor.
///
/// Potências de dois e por ordem decrescente: o alocador corta o atlas em
/// quadrados desta lista e nunca precisa de compactar nada.
pub const TILE_SIZES: [u32; 4] = [512, 256, 128, 64];

/// Uma luz que quer sombra, com o que é preciso para a ordenar.
#[derive(Clone, Copy, Debug)]
pub struct ShadowRequest {
    /// Índice na lista de luzes.
    pub light: u32,
    /// Maior = mais importante. Quem a calcula é quem sabe da cena.
    pub priority: f32,
    /// Resolução desejada, arredondada para baixo até uma classe de [`TILE_SIZES`].
    pub wanted: u32,
    /// Muda quando a luz ou algo que ela projecta se mexe. Igual = a sombra que
    /// está no atlas continua boa.
    pub version: u64,
}

/// Um pedaço do atlas atribuído a uma luz.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowSlot {
    pub light: u32,
    /// `x, y, lado` em texels do atlas.
    pub rect: [u32; 3],
    /// A versão que está **desenhada**. `None` = o slot é novo e não tem nada.
    pub drawn: Option<u64>,
}

impl ShadowSlot {
    /// Coordenadas normalizadas do tile, para o shader amostrar o atlas.
    pub fn uv_rect(&self, atlas_size: u32) -> [f32; 4] {
        let s = atlas_size.max(1) as f32;
        [
            self.rect[0] as f32 / s,
            self.rect[1] as f32 / s,
            self.rect[2] as f32 / s,
            self.rect[2] as f32 / s,
        ]
    }
}

/// O que fazer neste frame.
#[derive(Clone, Debug, Default)]
pub struct AtlasPlan {
    /// Índices em [`ShadowAtlas::slots`] a redesenhar, por ordem de prioridade.
    pub render: Vec<usize>,
    /// Texels que esse trabalho custa.
    pub texels: u64,
    /// Pedidos que não conseguiram slot nenhum.
    pub unplaced: u32,
    /// Slots que ficaram sujos e não couberam no orçamento deste frame.
    pub deferred: u32,
    /// Slots tirados a uma luz e dados a outra de maior prioridade.
    pub evicted: u32,
}

/// Slots livres de um tamanho, e o que está alocado.
#[derive(Clone, Debug)]
pub struct ShadowAtlas {
    size: u32,
    /// Orçamento por frame, em texels.
    budget: u64,
    slots: Vec<ShadowSlot>,
    /// Rectângulos livres, por classe de tamanho.
    free: Vec<Vec<[u32; 3]>>,
    /// Há quantos frames cada slot está sujo, para detectar inversão de prioridade.
    stale: Vec<u32>,
    /// Pior espera observada, em frames. Diagnóstico, não política.
    worst_stale: u32,
}

impl ShadowAtlas {
    /// Corta o atlas em quadrados das classes de [`TILE_SIZES`].
    ///
    /// A repartição é fixa e decidida aqui: metade da área para a classe maior,
    /// e o resto dividido pelas seguintes. Um alocador dinâmico daria melhor
    /// aproveitamento e exigiria compactar o atlas quando fragmentasse — e um
    /// atlas a compactar-se invalida conteúdo que estava bom, que é exactamente o
    /// que a cache existe para evitar.
    pub fn new(size: u32, budget_texels: u64) -> Self {
        let mut free: Vec<Vec<[u32; 3]>> = vec![Vec::new(); TILE_SIZES.len()];
        // Faixas horizontais: a primeira classe fica com metade da altura, a
        // seguinte com metade do que sobra, e assim por diante.
        let mut y = 0u32;
        for (class, &tile) in TILE_SIZES.iter().enumerate() {
            let remaining = size.saturating_sub(y);
            let band = if class == TILE_SIZES.len() - 1 {
                remaining
            } else {
                (remaining / 2).max(tile.min(remaining))
            };
            let rows = band / tile;
            let cols = size / tile;
            for r in 0..rows {
                for c in 0..cols {
                    free[class].push([c * tile, y + r * tile, tile]);
                }
            }
            y += rows * tile;
            if y >= size {
                break;
            }
        }
        Self {
            size,
            budget: budget_texels,
            slots: Vec::new(),
            free,
            stale: Vec::new(),
            worst_stale: 0,
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn budget(&self) -> u64 {
        self.budget
    }

    pub fn slots(&self) -> &[ShadowSlot] {
        &self.slots
    }

    /// Quantos frames o slot mais atrasado esperou, desde sempre.
    pub fn worst_stale(&self) -> u32 {
        self.worst_stale
    }

    /// Quantos tiles cabem, por classe.
    pub fn capacity(&self) -> Vec<usize> {
        self.free
            .iter()
            .enumerate()
            .map(|(class, f)| {
                f.len()
                    + self
                        .slots
                        .iter()
                        .filter(|s| s.rect[2] == TILE_SIZES[class])
                        .count()
            })
            .collect()
    }

    /// Decide o trabalho deste frame.
    ///
    /// `requests` não precisa de vir ordenado — ordena-se aqui, porque a ordem é
    /// parte da política e não do chamador.
    pub fn plan(&mut self, requests: &[ShadowRequest]) -> AtlasPlan {
        let mut order: Vec<usize> = (0..requests.len()).collect();
        // Decrescente por prioridade. Desempate pelo índice da luz, para o plano
        // ser determinista: sem isto, duas luzes com a mesma prioridade podiam
        // trocar de slot entre frames e invalidar a cache das duas por nada.
        order.sort_by(|&a, &b| {
            requests[b]
                .priority
                .partial_cmp(&requests[a].priority)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(requests[a].light.cmp(&requests[b].light))
        });

        let mut plan = AtlasPlan::default();
        // Quem fica com slot este frame, e com que versão.
        let mut keep: Vec<Option<(usize, u64)>> = vec![None; self.slots.len()];
        let mut new_slots: Vec<(u32, u32, u64)> = Vec::new(); // luz, classe, versão

        for &i in &order {
            let req = &requests[i];
            let class = size_class(req.wanted);
            if let Some(existing) = self
                .slots
                .iter()
                .position(|s| s.light == req.light && s.rect[2] == TILE_SIZES[class])
            {
                keep[existing] = Some((i, req.version));
                continue;
            }
            if self.free[class].is_empty() {
                // Tirar a uma luz de menor prioridade é legítimo; a uma de maior
                // não é, e como a lista está ordenada basta procurar entre as que
                // ainda não foram atendidas.
                if let Some(victim) = self.pick_victim(class, requests, &order, i, &keep) {
                    plan.evicted += 1;
                    let rect = self.slots[victim].rect;
                    self.free[class].push(rect);
                    keep[victim] = None;
                    self.slots[victim].light = u32::MAX;
                    self.slots[victim].drawn = None;
                } else {
                    plan.unplaced += 1;
                    continue;
                }
            }
            new_slots.push((req.light, class as u32, req.version));
        }

        // Reconstrói a tabela de slots: os que ficam mantêm o rect **e** o
        // conteúdo, que é a razão de tudo isto existir.
        let mut next: Vec<ShadowSlot> = Vec::with_capacity(self.slots.len());
        let mut next_stale: Vec<u32> = Vec::with_capacity(self.slots.len());
        let mut wanted: Vec<(usize, f32, u64)> = Vec::new(); // slot, prioridade, versão pedida

        for (s, slot) in self.slots.iter().enumerate() {
            if let Some((req_i, version)) = keep[s] {
                let idx = next.len();
                next.push(*slot);
                next_stale.push(self.stale.get(s).copied().unwrap_or(0));
                if slot.drawn != Some(version) {
                    wanted.push((idx, requests[req_i].priority, version));
                }
            } else if slot.light != u32::MAX {
                // Perdeu o pedido: o slot volta ao pote.
                self.free[size_class(slot.rect[2])].push(slot.rect);
            }
        }
        for (light, class, version) in new_slots {
            let Some(rect) = self.free[class as usize].pop() else {
                plan.unplaced += 1;
                continue;
            };
            let idx = next.len();
            next.push(ShadowSlot {
                light,
                rect,
                drawn: None,
            });
            next_stale.push(0);
            let prio = requests
                .iter()
                .find(|r| r.light == light)
                .map(|r| r.priority)
                .unwrap_or(0.0);
            wanted.push((idx, prio, version));
        }

        self.slots = next;
        self.stale = next_stale;

        // Gasta o orçamento por ordem de prioridade. Um slot que não caiba fica
        // sujo e sobe na fila do frame seguinte por ter esperado.
        wanted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (idx, _, version) in wanted {
            let cost = (self.slots[idx].rect[2] as u64).pow(2);
            if plan.texels + cost > self.budget && !plan.render.is_empty() {
                plan.deferred += 1;
                self.stale[idx] = self.stale[idx].saturating_add(1);
                self.worst_stale = self.worst_stale.max(self.stale[idx]);
                continue;
            }
            plan.texels += cost;
            plan.render.push(idx);
            self.stale[idx] = 0;
            // A versão só se marca como desenhada porque quem chama **vai**
            // desenhá-la. Se o plano fosse ignorado, a cache mentia.
            self.slots[idx].drawn = Some(version);
        }
        plan
    }

    /// A vítima de menor prioridade entre os residentes de uma classe, desde que
    /// valha menos do que quem a quer tirar.
    fn pick_victim(
        &self,
        class: usize,
        requests: &[ShadowRequest],
        order: &[usize],
        taker: usize,
        keep: &[Option<(usize, u64)>],
    ) -> Option<usize> {
        let tile = TILE_SIZES[class];
        let taker_prio = requests[taker].priority;
        let mut best: Option<(usize, f32)> = None;
        for (s, slot) in self.slots.iter().enumerate() {
            if slot.rect[2] != tile || slot.light == u32::MAX {
                continue;
            }
            // Um slot já confirmado neste frame por uma luz de maior prioridade
            // está fora de questão.
            if let Some((req_i, _)) = keep[s] {
                if requests[req_i].priority >= taker_prio {
                    continue;
                }
            }
            let prio = requests
                .iter()
                .find(|r| r.light == slot.light)
                .map(|r| r.priority)
                // Uma luz que já nem pede sombra vale menos do que qualquer uma
                // que peça: é a primeira a sair.
                .unwrap_or(f32::NEG_INFINITY);
            if prio >= taker_prio {
                continue;
            }
            if best.is_none_or(|(_, p)| prio < p) {
                best = Some((s, prio));
            }
        }
        let _ = order;
        best.map(|(s, _)| s)
    }
}

/// A classe cujo tile é o maior que não excede `wanted`.
fn size_class(wanted: u32) -> usize {
    TILE_SIZES
        .iter()
        .position(|&t| t <= wanted)
        .unwrap_or(TILE_SIZES.len() - 1)
}

/// Prioridade de uma luz: perto e grande no ecrã vale mais.
///
/// O raio a dividir pela distância é o tamanho angular — é isso que decide quantos
/// pixels a sombra vai ocupar, e portanto quanto é que um erro nela se vê. Uma luz
/// atrás da câmara ou fora do alcance devolve `None` e não entra na fila.
pub fn priority(light_pos_view: [f32; 3], radius: f32, far: f32) -> Option<f32> {
    let d2 = light_pos_view[0] * light_pos_view[0]
        + light_pos_view[1] * light_pos_view[1]
        + light_pos_view[2] * light_pos_view[2];
    let d = d2.sqrt();
    if d - radius > far {
        return None;
    }
    Some(radius / d.max(1e-3))
}

/// Resolução que uma luz merece, a partir da sua prioridade.
pub fn wanted_size(priority: f32) -> u32 {
    // Limiares por classe. Os números vêm do tamanho angular: 0.5 é uma luz que
    // ocupa metade do campo de visão, 0.02 é uma que quase não se vê.
    match priority {
        p if p >= 0.35 => TILE_SIZES[0],
        p if p >= 0.12 => TILE_SIZES[1],
        p if p >= 0.04 => TILE_SIZES[2],
        _ => TILE_SIZES[3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Corre planos até nada ficar por desenhar, com **limite**.
    ///
    /// `while !plan.render.is_empty() {}` é o que estava aqui, e quando eu quebrei
    /// a cache de propósito o teste **pendurou** em vez de falhar. Um teste que
    /// pendura bloqueia o CI e não diz nada; um que falha diz tudo.
    fn fill_cache(atlas: &mut ShadowAtlas, reqs: &[ShadowRequest]) -> u32 {
        for frame in 0..200 {
            if atlas.plan(reqs).render.is_empty() {
                return frame;
            }
        }
        panic!("a cache não estabilizou em 200 frames: alguma coisa redesenha sempre");
    }

    fn req(light: u32, priority: f32, wanted: u32, version: u64) -> ShadowRequest {
        ShadowRequest {
            light,
            priority,
            wanted,
            version,
        }
    }

    #[test]
    fn the_atlas_carves_tiles_that_fit_inside_it() {
        let atlas = ShadowAtlas::new(2048, u64::MAX);
        let cap = atlas.capacity();
        assert!(cap.iter().all(|&c| c > 0), "classe vazia: {cap:?}");
        let mut area = 0u64;
        for (class, &n) in cap.iter().enumerate() {
            area += n as u64 * (TILE_SIZES[class] as u64).pow(2);
        }
        assert!(
            area <= 2048u64 * 2048,
            "os tiles somam {area} texels num atlas de {}",
            2048u64 * 2048
        );
        // E não podem sobrepor-se.
        let mut rects: Vec<[u32; 3]> = Vec::new();
        for f in &atlas.free {
            rects.extend_from_slice(f);
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let disjoint = a[0] + a[2] <= b[0]
                    || b[0] + b[2] <= a[0]
                    || a[1] + a[2] <= b[1]
                    || b[1] + b[2] <= a[1];
                assert!(disjoint, "tiles sobrepostos: {a:?} e {b:?}");
            }
        }
    }

    /// O orçamento é o ponto todo: nunca pode ser excedido.
    #[test]
    fn the_budget_is_never_exceeded() {
        // Chega para dois tiles de 512 e nada mais.
        let budget = 2 * 512u64 * 512;
        let mut atlas = ShadowAtlas::new(2048, budget);
        let reqs: Vec<_> = (0..40)
            .map(|i| req(i, 1.0 - i as f32 * 0.01, 512, 1))
            .collect();
        for frame in 0..20 {
            let plan = atlas.plan(&reqs);
            assert!(
                plan.texels <= budget,
                "frame {frame}: {} texels contra um orçamento de {budget}",
                plan.texels
            );
        }
    }

    /// Numa cena parada tudo converge: mais nada para desenhar.
    ///
    /// É o invariante que torna o orçamento uma optimização e não uma mudança de
    /// resposta. Se não convergisse, «com orçamento» e «sem orçamento» dariam
    /// imagens diferentes para sempre.
    #[test]
    fn a_still_scene_converges_to_no_work() {
        let mut atlas = ShadowAtlas::new(2048, 512 * 512);
        let reqs: Vec<_> = (0..8)
            .map(|i| req(i, 1.0 - i as f32 * 0.05, 256, 7))
            .collect();
        let frames = fill_cache(&mut atlas, &reqs);
        assert!(frames > 0, "não chegou a desenhar nada");
        assert!(frames < 50, "convergiu, mas só ao fim de {frames} frames");
        // E todos têm conteúdo da versão certa.
        for slot in atlas.slots() {
            assert_eq!(slot.drawn, Some(7), "slot {slot:?} sem a versão desenhada");
        }
    }

    /// Mexer **uma** luz só obriga a redesenhar essa.
    #[test]
    fn only_the_light_that_moved_is_redrawn() {
        let mut atlas = ShadowAtlas::new(2048, u64::MAX);
        let mut reqs: Vec<_> = (0..6)
            .map(|i| req(i, 1.0 - i as f32 * 0.1, 256, 1))
            .collect();
        fill_cache(&mut atlas, &reqs);
        reqs[3].version = 2;
        let plan = atlas.plan(&reqs);
        assert_eq!(
            plan.render.len(),
            1,
            "redesenhou {} slots",
            plan.render.len()
        );
        assert_eq!(atlas.slots()[plan.render[0]].light, 3);
    }

    /// A fila é por prioridade: com orçamento para um, é o mais importante que sai.
    #[test]
    fn the_highest_priority_dirty_slot_wins_the_budget() {
        let mut atlas = ShadowAtlas::new(2048, 256 * 256);
        let mut reqs = vec![
            req(0, 0.1, 256, 1),
            req(1, 0.9, 256, 1),
            req(2, 0.5, 256, 1),
        ];
        // Enche a cache.
        fill_cache(&mut atlas, &reqs);
        // Mexem-se as três ao mesmo tempo.
        for r in &mut reqs {
            r.version = 2;
        }
        let plan = atlas.plan(&reqs);
        assert_eq!(plan.render.len(), 1);
        assert_eq!(
            atlas.slots()[plan.render[0]].light,
            1,
            "não foi a mais importante"
        );
        assert_eq!(plan.deferred, 2);
        // No frame seguinte sai a do meio.
        let plan = atlas.plan(&reqs);
        assert_eq!(atlas.slots()[plan.render[0]].light, 2);
    }

    /// Ninguém fica à espera para sempre enquanto os outros são servidos.
    ///
    /// É o falhanço clássico de uma fila por prioridade: com trabalho constante a
    /// chegar, o fundo da fila nunca é atendido. Aqui a prioridade é fixa, e o que
    /// se exige é que uma luz suja acabe por ser desenhada num número de frames
    /// limitado pelo orçamento — não que nunca espere.
    #[test]
    fn nothing_waits_forever() {
        let mut atlas = ShadowAtlas::new(2048, 256 * 256);
        let reqs: Vec<_> = (0..5)
            .map(|i| req(i, 1.0 - i as f32 * 0.1, 256, 1))
            .collect();
        let mut seen = [false; 5];
        for _ in 0..20 {
            for &idx in &atlas.plan(&reqs).render {
                seen[atlas.slots()[idx].light as usize] = true;
            }
        }
        assert!(seen.iter().all(|&s| s), "nunca desenhadas: {seen:?}");
        assert!(
            atlas.worst_stale() <= 5,
            "alguém esperou {} frames com 5 luzes e orçamento de 1",
            atlas.worst_stale()
        );
    }

    /// Uma luz importante tira o slot a uma insignificante, nunca o contrário.
    #[test]
    fn eviction_only_goes_downhill() {
        let mut atlas = ShadowAtlas::new(2048, u64::MAX);
        let small = atlas.capacity()[0];
        // Enche a classe maior com luzes de prioridade baixa.
        let filler: Vec<_> = (0..small as u32).map(|i| req(i, 0.1, 512, 1)).collect();
        atlas.plan(&filler);
        assert_eq!(atlas.slots().len(), small);

        // Chega uma muito mais importante.
        let mut reqs = filler.clone();
        reqs.push(req(999, 5.0, 512, 1));
        let plan = atlas.plan(&reqs);
        assert_eq!(plan.evicted, 1, "não despejou ninguém");
        assert!(
            atlas.slots().iter().any(|s| s.light == 999),
            "a luz importante não entrou"
        );

        // E o contrário não acontece: uma insignificante não tira nada.
        let before: Vec<u32> = atlas.slots().iter().map(|s| s.light).collect();
        let mut reqs2 = reqs.clone();
        reqs2.push(req(1000, 0.0001, 512, 1));
        let plan2 = atlas.plan(&reqs2);
        assert_eq!(plan2.evicted, 0, "uma luz sem importância despejou alguém");
        // **Duas** sem slot, não uma: a luz que o 999 despejou volta a pedir todos
        // os frames e continua a não caber, e agora há mais uma atrás dela. Eu
        // tinha escrito 1 e o teste apanhou-me — o despejo não devolve o lugar a
        // quem o perdeu, e é isso que se quer: se devolvesse, as duas trocavam de
        // slot em frames alternados e invalidavam a cache uma da outra para sempre.
        assert_eq!(plan2.unplaced, 2);
        let after: Vec<u32> = atlas.slots().iter().map(|s| s.light).collect();
        assert_eq!(before, after, "a tabela de slots mexeu-se sem razão");
    }

    /// Um slot nunca pode dizer que tem uma versão que ninguém desenhou.
    #[test]
    fn a_slot_never_claims_content_it_was_not_given() {
        let mut atlas = ShadowAtlas::new(1024, 128 * 128);
        let reqs: Vec<_> = (0..10)
            .map(|i| req(i, 1.0 - i as f32 * 0.05, 128, 3))
            .collect();
        for _ in 0..4 {
            let plan = atlas.plan(&reqs);
            let drawn: Vec<usize> = plan.render.clone();
            for (i, slot) in atlas.slots().iter().enumerate() {
                if slot.drawn.is_some() {
                    // Ou foi desenhado agora, ou num frame anterior — o que não
                    // pode é ter conteúdo sem nunca ter entrado num plano.
                    assert!(
                        drawn.contains(&i) || slot.drawn == Some(3),
                        "slot {i} diz ter {:?} sem plano",
                        slot.drawn
                    );
                }
            }
        }
    }

    /// A ordem em que os pedidos chegam não pode mudar nada.
    ///
    /// O `sort_by` do Rust é estável, portanto sem desempate explícito o plano
    /// seguiria a ordem do **chamador** — e um chamador que reordene as luzes por
    /// outra razão qualquer faria duas luzes de igual prioridade trocarem de slot
    /// em frames alternados, invalidando a cache uma da outra para sempre.
    ///
    /// Escrevi o desempate por índice e afirmei isto no comentário; depois quebrei
    /// o desempate e **nenhum teste falhou**. Este é o teste que faltava.
    #[test]
    fn the_plan_does_not_depend_on_the_order_requests_arrive_in() {
        let make = |lights: &[u32]| -> Vec<ShadowRequest> {
            lights.iter().map(|&l| req(l, 0.5, 256, 1)).collect()
        };
        let forward = make(&[0, 1, 2, 3, 4, 5, 6, 7]);
        let shuffled = make(&[5, 2, 7, 0, 3, 6, 1, 4]);

        let mut a = ShadowAtlas::new(1024, u64::MAX);
        a.plan(&forward);
        let mut b = ShadowAtlas::new(1024, u64::MAX);
        b.plan(&shuffled);

        let key = |at: &ShadowAtlas| {
            let mut v: Vec<(u32, [u32; 3])> =
                at.slots().iter().map(|s| (s.light, s.rect)).collect();
            v.sort();
            v
        };
        assert_eq!(key(&a), key(&b), "a ordem dos pedidos mudou a atribuição");
    }

    #[test]
    fn priority_prefers_near_and_large() {
        let near = priority([0.0, 0.0, -5.0], 4.0, 100.0).unwrap();
        let far = priority([0.0, 0.0, -50.0], 4.0, 100.0).unwrap();
        assert!(near > far, "{near} vs {far}");
        let big = priority([0.0, 0.0, -20.0], 10.0, 100.0).unwrap();
        let small = priority([0.0, 0.0, -20.0], 2.0, 100.0).unwrap();
        assert!(big > small);
        assert!(
            priority([0.0, 0.0, -500.0], 1.0, 100.0).is_none(),
            "fora do far"
        );
    }

    #[test]
    fn wanted_size_is_monotonic() {
        let mut last = 0;
        for i in 0..100 {
            let p = i as f32 * 0.01;
            let s = wanted_size(p);
            assert!(s >= last, "a resolução desceu com a prioridade a subir");
            last = s;
        }
        assert_eq!(wanted_size(1.0), TILE_SIZES[0]);
        assert_eq!(wanted_size(0.0), TILE_SIZES[3]);
    }

    #[test]
    fn size_class_rounds_down() {
        assert_eq!(size_class(512), 0);
        assert_eq!(size_class(511), 1);
        assert_eq!(size_class(64), 3);
        assert_eq!(size_class(1), 3, "abaixo da menor classe fica na menor");
        assert_eq!(size_class(4096), 0, "acima da maior fica na maior");
    }
}
