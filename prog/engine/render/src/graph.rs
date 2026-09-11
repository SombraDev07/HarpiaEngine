//! Render graph: quem lê o quê, e as barreiras que daí saem.
//!
//! Até aqui cada sample sequenciava as suas passes à mão e as barreiras eram
//! raciocinadas caso a caso. Funcionou até às 29 passes que a árvore tem hoje, e
//! esta sessão deu três provas de que deixou de funcionar:
//!
//! * o `storage_barrier_buffer` teve de ser **alargado à mão** para cobrir um
//!   compute que escreve o que o compute seguinte lê — e o modo de falha sem isso
//!   é intermitente, que é o pior de todos (D48);
//! * `depth_clear: None` queria dizer «limpa na mesma», e ninguém via isso de onde
//!   se chamava. Foi preciso descobri-lo para a cache do atlas existir (D52);
//! * o atlas de sombras transita de *depth attachment* para *shader read* entre
//!   passes porque o RHI o faz implicitamente ao ligar. Funciona, e **nada
//!   verifica** que funciona.
//!
//! São três classes de erro que desaparecem se as passes **declararem** o que
//! usam e as barreiras forem derivadas daí.
//!
//! ## O que este grafo faz, e o que não faz
//!
//! Faz: valida o que foi declarado, deriva as barreiras entre acessos
//! consecutivos do mesmo recurso, e decide os `loadOp` das ligações de saída.
//!
//! **Não** faz aliasing de memória entre transientes nem reordena passes. As duas
//! coisas são optimizações e entram depois de isto estar provado — um grafo que
//! reordena e ainda não se sabe se derivou as barreiras certas é uma máquina de
//! bugs que ninguém consegue depurar.
//!
//! ## Porque é que isto não conhece Vulkan
//!
//! D0 diz que `vk::*` só existe em `prog/engine/drv`. O grafo fala em [`Access`]
//! e emite [`harpia_rhi::BarrierDesc`]; é o backend que traduz para estágios,
//! máscaras de acesso e layouts. A vantagem prática é que **toda a lógica que se
//! segue é testável sem GPU** — e é onde os testes deste módulo estão.

use harpia_rhi::{Barrier, BarrierDesc, Buffer, Texture};

/// O que uma pass faz a um recurso.
///
/// A lista é deliberadamente pequena. Cada entrada tem de corresponder a uma
/// combinação de estágio e máscara de acesso que o backend saiba traduzir, e
/// inventar variantes «por via das dúvidas» só cria caminhos que nunca são
/// exercitados.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Access {
    /// Escrito como ligação de cor.
    ColorWrite,
    /// Escrito como profundidade.
    DepthWrite,
    /// Testado contra profundidade, sem escrever.
    DepthRead,
    /// Amostrado num shader.
    Sampled,
    /// Lido por um binding de storage (buffer ou imagem).
    StorageRead,
    /// Escrito por um binding de storage.
    StorageWrite,
    /// Lido pelo comando de draw indirecto.
    Indirect,
    /// Entrada de vértices ou índices.
    VertexInput,
    /// Escrito pela CPU antes da pass (upload por staging).
    HostWrite,
}

impl Access {
    pub fn writes(self) -> bool {
        matches!(
            self,
            Access::ColorWrite | Access::DepthWrite | Access::StorageWrite | Access::HostWrite
        )
    }

    pub fn reads(self) -> bool {
        !self.writes() || self == Access::DepthWrite
    }

    /// O layout que uma imagem precisa de ter para este acesso.
    ///
    /// Buffers não têm layout; a função existe à mesma porque é a mudança de
    /// layout que obriga a uma barreira mesmo entre duas **leituras** — e é
    /// exactamente o caso do atlas de sombras, que passa de profundidade a
    /// amostrado sem ninguém escrever nada pelo meio.
    pub fn layout(self) -> Layout {
        match self {
            Access::ColorWrite => Layout::ColorAttachment,
            Access::DepthWrite => Layout::DepthAttachment,
            Access::DepthRead => Layout::DepthReadOnly,
            Access::Sampled => Layout::ShaderRead,
            Access::StorageRead | Access::StorageWrite => Layout::General,
            Access::Indirect | Access::VertexInput | Access::HostWrite => Layout::Undefined,
        }
    }
}

/// Layout de imagem, em termos neutros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Undefined,
    ColorAttachment,
    DepthAttachment,
    DepthReadOnly,
    ShaderRead,
    General,
}

/// Um recurso do grafo.
#[derive(Clone, Copy, Debug)]
pub struct Resource {
    pub name: &'static str,
    pub handle: Handle,
    /// O conteúdo de antes deste frame tem de sobreviver.
    ///
    /// É o que distingue um alvo temporário de uma cache: o atlas de sombras é
    /// persistente, o G-buffer não. Um recurso persistente pode ser lido sem
    /// ninguém o ter escrito **neste** frame; um transiente não, e isso é um erro
    /// que o grafo apanha em vez de desenhar lixo.
    pub persistent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Texture(Texture),
    Buffer(Buffer),
}

impl Handle {
    pub fn is_texture(self) -> bool {
        matches!(self, Handle::Texture(_))
    }
}

/// Índice de um recurso no grafo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceId(pub u32);

/// Índice de uma pass no grafo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct PassId(pub u32);

/// O que fazer ao conteúdo de uma ligação de saída quando a pass começa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Load {
    /// Limpa com este valor.
    Clear([f32; 4]),
    /// Carrega o que lá está. Só é válido se houver conteúdo — um recurso
    /// persistente, ou escrito antes neste frame. O grafo verifica.
    Keep,
    /// Ninguém quer o que lá está. O backend pode descartar.
    Discard,
}

/// Uma pass declarada.
#[derive(Clone, Debug)]
pub struct Pass {
    pub name: &'static str,
    uses: Vec<(ResourceId, Access)>,
    loads: Vec<(ResourceId, Load)>,
}

impl Pass {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            uses: Vec::new(),
            loads: Vec::new(),
        }
    }

    /// Declara um acesso. Encadeável.
    pub fn uses(mut self, res: ResourceId, access: Access) -> Self {
        self.uses.push((res, access));
        self
    }

    /// Declara o que fazer ao conteúdo de uma ligação de saída.
    ///
    /// Sem isto, uma ligação de cor ou profundidade é `Discard`: quem escreve por
    /// cima de tudo não precisa do que lá estava. `Keep` é o que a cache do atlas
    /// pede, e o grafo recusa-o se não houver conteúdo nenhum para guardar.
    pub fn load(mut self, res: ResourceId, load: Load) -> Self {
        self.loads.push((res, load));
        self
    }

    pub fn accesses(&self) -> &[(ResourceId, Access)] {
        &self.uses
    }
}

/// O grafo de um frame.
#[derive(Clone, Debug, Default)]
pub struct RenderGraph {
    resources: Vec<Resource>,
    passes: Vec<Pass>,
}

/// O que correu mal na declaração.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphError {
    /// Um transiente lido sem ninguém o ter escrito neste frame.
    ReadBeforeWrite {
        resource: &'static str,
        pass: &'static str,
    },
    /// `Load::Keep` num recurso que não tem conteúdo nenhum.
    KeepWithoutContent {
        resource: &'static str,
        pass: &'static str,
    },
    /// A mesma pass declara dois acessos incompatíveis ao mesmo recurso.
    ConflictingAccess {
        resource: &'static str,
        pass: &'static str,
        a: Access,
        b: Access,
    },
    /// Um `Load` declarado para um recurso que a pass não escreve.
    LoadWithoutWrite {
        resource: &'static str,
        pass: &'static str,
    },
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::ReadBeforeWrite { resource, pass } => write!(
                f,
                "a pass `{pass}` lê `{resource}` e ninguém o escreveu neste frame; \
                 se o conteúdo vem de antes, marca o recurso como persistente"
            ),
            GraphError::KeepWithoutContent { resource, pass } => write!(
                f,
                "a pass `{pass}` pede `Load::Keep` em `{resource}`, que não tem \
                 conteúdo nenhum para guardar"
            ),
            GraphError::ConflictingAccess {
                resource,
                pass,
                a,
                b,
            } => write!(
                f,
                "a pass `{pass}` declara {a:?} e {b:?} sobre `{resource}` ao mesmo \
                 tempo, e os dois pedem layouts diferentes"
            ),
            GraphError::LoadWithoutWrite { resource, pass } => write!(
                f,
                "a pass `{pass}` declara um `Load` para `{resource}` sem o escrever"
            ),
        }
    }
}

/// O plano de uma pass: o que fazer **antes** dela, e com que `loadOp`.
#[derive(Clone, Debug, Default)]
pub struct PassPlan {
    pub pass: PassId,
    /// Barreiras a emitir antes de a pass começar.
    pub barriers: Vec<BarrierDesc>,
    /// `loadOp` resolvido por ligação de saída.
    pub loads: Vec<(ResourceId, Load)>,
}

impl RenderGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Regista uma textura transiente (o conteúdo não sobrevive ao frame).
    pub fn texture(&mut self, name: &'static str, tex: Texture) -> ResourceId {
        self.add(name, Handle::Texture(tex), false)
    }

    /// Regista uma textura cujo conteúdo sobrevive entre frames.
    pub fn persistent_texture(&mut self, name: &'static str, tex: Texture) -> ResourceId {
        self.add(name, Handle::Texture(tex), true)
    }

    pub fn buffer(&mut self, name: &'static str, buf: Buffer) -> ResourceId {
        self.add(name, Handle::Buffer(buf), false)
    }

    pub fn persistent_buffer(&mut self, name: &'static str, buf: Buffer) -> ResourceId {
        self.add(name, Handle::Buffer(buf), true)
    }

    fn add(&mut self, name: &'static str, handle: Handle, persistent: bool) -> ResourceId {
        self.resources.push(Resource {
            name,
            handle,
            persistent,
        });
        ResourceId(self.resources.len() as u32 - 1)
    }

    pub fn pass(&mut self, pass: Pass) -> PassId {
        self.passes.push(pass);
        PassId(self.passes.len() as u32 - 1)
    }

    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }

    pub fn passes(&self) -> &[Pass] {
        &self.passes
    }

    /// Valida a declaração. Devolve **todos** os erros, não o primeiro.
    ///
    /// Todos e não o primeiro porque quem está a converter uma cena para o grafo
    /// quer a lista, não um jogo de descasca-cebola com uma recompilação por erro.
    pub fn validate(&self) -> Result<(), Vec<GraphError>> {
        let mut errors = Vec::new();
        // Quem já escreveu cada recurso, por ordem de pass.
        let mut written: Vec<bool> = self.resources.iter().map(|r| r.persistent).collect();

        for pass in &self.passes {
            // Dois acessos ao mesmo recurso na mesma pass só são compatíveis se
            // pedirem o mesmo layout.
            for (i, &(res_a, a)) in pass.uses.iter().enumerate() {
                for &(res_b, b) in &pass.uses[i + 1..] {
                    if res_a == res_b && a.layout() != b.layout() {
                        errors.push(GraphError::ConflictingAccess {
                            resource: self.name(res_a),
                            pass: pass.name,
                            a,
                            b,
                        });
                    }
                }
            }
            for &(res, access) in &pass.uses {
                let idx = res.0 as usize;
                if access.writes() {
                    continue;
                }
                if !written.get(idx).copied().unwrap_or(false) {
                    errors.push(GraphError::ReadBeforeWrite {
                        resource: self.name(res),
                        pass: pass.name,
                    });
                }
            }
            for &(res, load) in &pass.loads {
                let idx = res.0 as usize;
                let writes_it = pass.uses.iter().any(|&(r, a)| {
                    r == res && (a == Access::ColorWrite || a == Access::DepthWrite)
                });
                if !writes_it {
                    errors.push(GraphError::LoadWithoutWrite {
                        resource: self.name(res),
                        pass: pass.name,
                    });
                    continue;
                }
                if load == Load::Keep && !written.get(idx).copied().unwrap_or(false) {
                    errors.push(GraphError::KeepWithoutContent {
                        resource: self.name(res),
                        pass: pass.name,
                    });
                }
            }
            // Só depois de validar a pass é que as escritas dela contam, senão
            // uma pass que lê e escreve o mesmo recurso validava-se a si própria.
            for &(res, access) in &pass.uses {
                if access.writes() {
                    if let Some(w) = written.get_mut(res.0 as usize) {
                        *w = true;
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn name(&self, res: ResourceId) -> &'static str {
        self.resources
            .get(res.0 as usize)
            .map_or("<desconhecido>", |r| r.name)
    }

    /// Deriva as barreiras e os `loadOp`, pass a pass.
    ///
    /// A regra é uma só: entre dois acessos consecutivos ao mesmo recurso há
    /// barreira se **algum deles escreve** ou se o **layout muda**. Duas leituras
    /// no mesmo layout não precisam de nada — e é isso que impede o grafo de
    /// encher o frame de barreiras inúteis, que seria a maneira fácil de estar
    /// sempre correcto e sempre lento.
    pub fn compile(&self) -> Vec<PassPlan> {
        // Último acesso de cada recurso: (pass, acesso).
        let mut last: Vec<Option<Access>> = vec![None; self.resources.len()];
        let mut plans = Vec::with_capacity(self.passes.len());

        for (p, pass) in self.passes.iter().enumerate() {
            let mut plan = PassPlan {
                pass: PassId(p as u32),
                ..Default::default()
            };
            // Ordenado por recurso para o plano ser determinista: a ordem das
            // barreiras não muda o resultado, mas muda o que um teste compara.
            let mut uses: Vec<(ResourceId, Access)> = pass.uses.clone();
            uses.sort_by_key(|&(r, _)| r);
            uses.dedup();

            for (res, access) in uses {
                let idx = res.0 as usize;
                let prev = last.get(idx).copied().flatten();
                if let Some(prev) = prev {
                    let layout_change =
                        self.resources[idx].handle.is_texture() && prev.layout() != access.layout();
                    if prev.writes() || access.writes() || layout_change {
                        plan.barriers.push(BarrierDesc {
                            resource: match self.resources[idx].handle {
                                Handle::Texture(t) => Barrier::Texture(t),
                                Handle::Buffer(b) => Barrier::Buffer(b),
                            },
                            src: access_code(prev),
                            dst: access_code(access),
                        });
                    }
                } else if self.resources[idx].handle.is_texture() {
                    // Primeiro toque num recurso: o layout inicial é desconhecido
                    // e tem de ser posto, senão o backend lê de um layout que
                    // ninguém definiu.
                    plan.barriers.push(BarrierDesc {
                        resource: match self.resources[idx].handle {
                            Handle::Texture(t) => Barrier::Texture(t),
                            Handle::Buffer(b) => Barrier::Buffer(b),
                        },
                        src: access_code(Access::HostWrite),
                        dst: access_code(access),
                    });
                }
                if let Some(slot) = last.get_mut(idx) {
                    *slot = Some(access);
                }
            }

            let mut loads = pass.loads.clone();
            loads.sort_by_key(|&(r, _)| r);
            plan.loads = loads;
            plans.push(plan);
        }
        plans
    }
}

/// Código neutro que o backend traduz para estágio, acesso e layout.
///
/// Um `u32` e não o `Access` directamente porque o `harpia-rhi` não pode depender
/// do `harpia-render` — a seta das dependências vai ao contrário.
fn access_code(a: Access) -> u32 {
    match a {
        Access::ColorWrite => 0,
        Access::DepthWrite => 1,
        Access::DepthRead => 2,
        Access::Sampled => 3,
        Access::StorageRead => 4,
        Access::StorageWrite => 5,
        Access::Indirect => 6,
        Access::VertexInput => 7,
        Access::HostWrite => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(id: u32) -> Texture {
        Texture::from_raw(id)
    }

    fn buf(id: u32) -> Buffer {
        Buffer::from_raw(id)
    }

    /// O caso que motivou tudo isto: compute escreve, compute lê, draw indirecto
    /// lê. Duas barreiras, e nenhuma delas escrita à mão.
    #[test]
    fn compute_to_compute_to_indirect_gets_both_barriers() {
        let mut g = RenderGraph::new();
        let bounds = g.buffer("bounds", buf(1));
        let args = g.buffer("args", buf(2));
        g.pass(Pass::new("bounds").uses(bounds, Access::StorageWrite));
        g.pass(
            Pass::new("cull")
                .uses(bounds, Access::StorageRead)
                .uses(args, Access::StorageWrite),
        );
        g.pass(Pass::new("draw").uses(args, Access::Indirect));

        g.validate().expect("grafo válido");
        let plans = g.compile();
        assert!(
            plans[0].barriers.is_empty(),
            "a primeira pass não espera por nada"
        );
        assert_eq!(plans[1].barriers.len(), 1, "falta a barreira bounds→cull");
        assert_eq!(plans[2].barriers.len(), 1, "falta a barreira cull→draw");
    }

    /// O atlas de sombras: escrito como profundidade, depois amostrado.
    ///
    /// É a barreira que hoje acontece por acidente porque o RHI a faz
    /// implicitamente ao ligar, sem nada verificar que a fez.
    #[test]
    fn writing_depth_then_sampling_it_gets_a_barrier() {
        let mut g = RenderGraph::new();
        let atlas = g.persistent_texture("atlas", tex(1));
        let out = g.texture("out", tex(2));
        g.pass(Pass::new("shadows").uses(atlas, Access::DepthWrite));
        g.pass(
            Pass::new("lighting")
                .uses(atlas, Access::Sampled)
                .uses(out, Access::ColorWrite),
        );
        g.validate().expect("válido");
        let plans = g.compile();
        assert_eq!(
            plans[1].barriers.len(),
            2,
            "atlas (layout) e out (primeiro toque)"
        );
    }

    /// Duas **leituras** com layouts diferentes precisam de barreira à mesma.
    ///
    /// Este teste existe porque o outro, que eu tinha chamado «mesmo entre duas
    /// leituras», não testava isso: a sequência lá era `DepthWrite → Sampled`, e
    /// a barreira saía pelo ramo da escrita. Tirei o teste de layout do código e
    /// **nenhum teste falhou**. Aqui a profundidade é lida como teste numa pass e
    /// amostrada na seguinte — ninguém escreve, e o layout muda à mesma.
    #[test]
    fn two_reads_with_different_layouts_still_need_a_barrier() {
        let mut g = RenderGraph::new();
        let depth = g.persistent_texture("depth", tex(1));
        let a = g.texture("a", tex(2));
        let b = g.texture("b", tex(3));
        g.pass(
            Pass::new("forward")
                .uses(depth, Access::DepthRead)
                .uses(a, Access::ColorWrite),
        );
        g.pass(
            Pass::new("fog")
                .uses(depth, Access::Sampled)
                .uses(b, Access::ColorWrite),
        );
        g.validate().expect("válido");
        let plans = g.compile();
        let on_depth = plans[1]
            .barriers
            .iter()
            .filter(|b| matches!(b.resource, Barrier::Texture(x) if x.raw() == 1))
            .count();
        assert_eq!(
            on_depth, 1,
            "DepthReadOnly → ShaderRead é uma mudança de layout e precisa de barreira"
        );
    }

    /// Duas leituras no mesmo layout não precisam de barreira nenhuma.
    ///
    /// Sem esta regra o grafo estaria sempre correcto e sempre lento, que é a
    /// maneira fácil de fingir que funciona.
    #[test]
    fn two_reads_in_the_same_layout_need_nothing() {
        let mut g = RenderGraph::new();
        let t = g.persistent_texture("t", tex(1));
        let a = g.texture("a", tex(2));
        let b = g.texture("b", tex(3));
        g.pass(
            Pass::new("um")
                .uses(t, Access::Sampled)
                .uses(a, Access::ColorWrite),
        );
        g.pass(
            Pass::new("dois")
                .uses(t, Access::Sampled)
                .uses(b, Access::ColorWrite),
        );
        g.validate().expect("válido");
        let plans = g.compile();
        let on_t = plans[1]
            .barriers
            .iter()
            .filter(|b| matches!(b.resource, Barrier::Texture(x) if x.raw() == 1))
            .count();
        assert_eq!(
            on_t, 0,
            "duas amostragens seguidas não precisam de barreira"
        );
    }

    /// Ler um transiente que ninguém escreveu é um erro, não lixo no ecrã.
    #[test]
    fn reading_a_transient_nobody_wrote_is_an_error() {
        let mut g = RenderGraph::new();
        let t = g.texture("gbuffer", tex(1));
        let out = g.texture("out", tex(2));
        g.pass(
            Pass::new("lighting")
                .uses(t, Access::Sampled)
                .uses(out, Access::ColorWrite),
        );
        let errors = g.validate().expect_err("devia falhar");
        assert_eq!(
            errors,
            vec![GraphError::ReadBeforeWrite {
                resource: "gbuffer",
                pass: "lighting"
            }]
        );
    }

    /// O mesmo recurso marcado persistente já não é erro: o conteúdo vem de antes.
    #[test]
    fn a_persistent_resource_may_be_read_first() {
        let mut g = RenderGraph::new();
        let t = g.persistent_texture("history", tex(1));
        let out = g.texture("out", tex(2));
        g.pass(
            Pass::new("taa")
                .uses(t, Access::Sampled)
                .uses(out, Access::ColorWrite),
        );
        g.validate().expect("persistente pode ser lido primeiro");
    }

    /// `Load::Keep` sem conteúdo é o erro que o `depth_clear: None` escondia.
    #[test]
    fn keeping_content_that_does_not_exist_is_an_error() {
        let mut g = RenderGraph::new();
        let d = g.texture("depth", tex(1));
        g.pass(
            Pass::new("sombras")
                .uses(d, Access::DepthWrite)
                .load(d, Load::Keep),
        );
        let errors = g.validate().expect_err("devia falhar");
        assert_eq!(
            errors,
            vec![GraphError::KeepWithoutContent {
                resource: "depth",
                pass: "sombras"
            }]
        );
    }

    /// E com o atlas marcado persistente, `Keep` é exactamente o que a cache quer.
    #[test]
    fn a_persistent_atlas_may_keep_its_content() {
        let mut g = RenderGraph::new();
        let d = g.persistent_texture("atlas", tex(1));
        g.pass(
            Pass::new("sombras")
                .uses(d, Access::DepthWrite)
                .load(d, Load::Keep),
        );
        g.validate().expect("a cache pode guardar o que tem");
    }

    #[test]
    fn a_load_for_a_resource_the_pass_does_not_write_is_an_error() {
        let mut g = RenderGraph::new();
        let t = g.persistent_texture("t", tex(1));
        let out = g.texture("out", tex(2));
        g.pass(
            Pass::new("p")
                .uses(t, Access::Sampled)
                .uses(out, Access::ColorWrite)
                .load(t, Load::Keep),
        );
        let errors = g.validate().expect_err("devia falhar");
        assert_eq!(
            errors,
            vec![GraphError::LoadWithoutWrite {
                resource: "t",
                pass: "p"
            }]
        );
    }

    /// Dois acessos ao mesmo recurso na mesma pass com layouts diferentes.
    #[test]
    fn conflicting_access_in_one_pass_is_an_error() {
        let mut g = RenderGraph::new();
        let t = g.persistent_texture("t", tex(1));
        g.pass(
            Pass::new("p")
                .uses(t, Access::ColorWrite)
                .uses(t, Access::Sampled),
        );
        let errors = g.validate().expect_err("devia falhar");
        assert!(matches!(errors[0], GraphError::ConflictingAccess { .. }));
    }

    /// A validação devolve **todos** os erros, não só o primeiro.
    #[test]
    fn validation_reports_every_error_at_once() {
        let mut g = RenderGraph::new();
        let a = g.texture("a", tex(1));
        let b = g.texture("b", tex(2));
        let out = g.texture("out", tex(3));
        g.pass(
            Pass::new("p")
                .uses(a, Access::Sampled)
                .uses(b, Access::Sampled)
                .uses(out, Access::ColorWrite),
        );
        let errors = g.validate().expect_err("devia falhar");
        assert_eq!(errors.len(), 2, "só reportou {} de 2", errors.len());
    }

    /// **A propriedade que interessa**: todo o par escrita→leitura do mesmo
    /// recurso tem uma barreira entre as duas passes.
    ///
    /// É isto que substitui o raciocínio caso a caso. Percorre-se o grafo
    /// inteiro, procura-se cada par, e exige-se a barreira.
    #[test]
    fn every_write_then_read_has_a_barrier_between_them() {
        let mut g = RenderGraph::new();
        let depth = g.texture("depth", tex(1));
        let gbuf = g.texture("gbuf", tex(2));
        let lit = g.texture("lit", tex(3));
        let hist = g.persistent_texture("hist", tex(4));
        let lights = g.buffer("lights", buf(1));
        let args = g.buffer("args", buf(2));

        g.pass(Pass::new("upload").uses(lights, Access::HostWrite));
        g.pass(
            Pass::new("cull")
                .uses(lights, Access::StorageRead)
                .uses(args, Access::StorageWrite),
        );
        g.pass(
            Pass::new("gbuffer")
                .uses(gbuf, Access::ColorWrite)
                .uses(depth, Access::DepthWrite)
                .uses(args, Access::Indirect),
        );
        g.pass(
            Pass::new("lighting")
                .uses(gbuf, Access::Sampled)
                .uses(depth, Access::DepthRead)
                .uses(lights, Access::StorageRead)
                .uses(lit, Access::ColorWrite),
        );
        g.pass(
            Pass::new("taa")
                .uses(lit, Access::Sampled)
                .uses(hist, Access::ColorWrite),
        );
        g.validate().expect("válido");

        let plans = g.compile();
        // Reconstrói o histórico de acessos e confirma cada par.
        let mut last: Vec<Option<(usize, Access)>> = vec![None; g.resources().len()];
        for (p, pass) in g.passes().iter().enumerate() {
            for &(res, access) in pass.accesses() {
                if let Some((prev_pass, prev)) = last[res.0 as usize] {
                    let needs = prev.writes()
                        || access.writes()
                        || (g.resources()[res.0 as usize].handle.is_texture()
                            && prev.layout() != access.layout());
                    if needs {
                        let found = plans[p].barriers.iter().any(|b| match b.resource {
                            Barrier::Texture(t) => {
                                matches!(g.resources()[res.0 as usize].handle, Handle::Texture(x) if x.raw() == t.raw())
                            }
                            Barrier::Buffer(x) => {
                                matches!(g.resources()[res.0 as usize].handle, Handle::Buffer(y) if y.raw() == x.raw())
                            }
                        });
                        assert!(
                            found,
                            "sem barreira para `{}` entre a pass {prev_pass} e a {p}",
                            g.resources()[res.0 as usize].name
                        );
                    }
                }
                last[res.0 as usize] = Some((p, access));
            }
        }
    }

    /// O plano não pode depender da ordem em que os acessos foram declarados.
    #[test]
    fn the_plan_does_not_depend_on_declaration_order() {
        let build = |swap: bool| {
            let mut g = RenderGraph::new();
            let a = g.persistent_texture("a", tex(1));
            let b = g.persistent_texture("b", tex(2));
            let out = g.texture("out", tex(3));
            let p = if swap {
                Pass::new("p")
                    .uses(b, Access::Sampled)
                    .uses(a, Access::Sampled)
                    .uses(out, Access::ColorWrite)
            } else {
                Pass::new("p")
                    .uses(a, Access::Sampled)
                    .uses(b, Access::Sampled)
                    .uses(out, Access::ColorWrite)
            };
            g.pass(p);
            g.compile()
        };
        let x = build(false);
        let y = build(true);
        let key = |plans: &[PassPlan]| {
            plans
                .iter()
                .map(|p| {
                    p.barriers
                        .iter()
                        .map(|b| (format!("{:?}", b.resource), b.src, b.dst))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&x), key(&y));
    }
}
